//! Knowledge hooks (`kh:`): RDF-described derivation rules, condition evaluation,
//! automated repair, and SQL-in-RDF integration.

pub mod cache;
pub mod dependency_graph;

use std::collections::BTreeSet;
use std::sync::Arc;

use purrdf_core::diagnostic::RdfDiagnostic;
use purrdf_core::{
    backend::TermFactory, RdfDataset, RdfDatasetBuilder, SparqlEngine, SparqlRequest, SparqlResult,
    TermValue,
};
use purrdf_duckdb::PurRdfDuckDB;
use purrdf_shapes::engine::{parse_shapes, validate_dataset};
use purrdf_sparql_eval::NativeSparqlEngine;
use purrdf_sql::compile_turtle;
use thiserror::Error;

pub use cache::ConditionCache;
pub use dependency_graph::DependencyGraph;

/// The `kh:` vocabulary namespace.
pub const KH: &str = "http://seanchatmangpt.github.io/graphlaw/kh#";
/// The supported action handler for SPARQL CONSTRUCT.
pub const HANDLER_SPARQL_CONSTRUCT: &str =
    "http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct";
/// The supported action handler for SQL CONSTRUCT (relational to RDF projection).
pub const HANDLER_SQL_CONSTRUCT: &str =
    "http://seanchatmangpt.github.io/graphlaw/handler#sql-construct";
/// Hard ceiling on fixpoint rounds.
pub const MAX_ROUNDS: usize = 64;
/// Hard ceiling on total hook firings.
pub const MAX_FIRINGS: usize = 10_000;
/// Largest dataset, in quads, a pack may grow the state to.
pub const MAX_STATE_QUADS: usize = 1_000_000;

/// Errors arising from knowledge hook execution.
#[derive(Debug, Error)]
pub enum HookError {
    /// SPARQL evaluation or parsing error.
    #[error("SPARQL error: {0:?}")]
    Sparql(RdfDiagnostic),
    /// Underlying RDF dataset or term storage error.
    #[error("RDF error: {0}")]
    Rdf(String),
    /// Unsupported hook configuration or invalid handler.
    #[error("Unsupported hook configuration: {0}")]
    Unsupported(String),
    /// Resource limit exceeded during materialization.
    #[error("Resource limit exceeded: {0}")]
    ResourceLimit(String),
    /// Dependency cycle detected among hooks.
    #[error("Dependency cycle detected: {0:?}")]
    Cycle(Vec<String>),
    /// SHACL shapes evaluation error.
    #[error("SHACL error: {0}")]
    Shacl(String),
    /// SQL-in-RDF compilation error.
    #[error("SQL AST error: {0}")]
    Sql(#[from] purrdf_sql::AstError),
    /// DuckDB execution error.
    #[error("DuckDB evaluation error: {0}")]
    DuckDb(#[from] purrdf_duckdb::EvaluatorError),
}

impl From<RdfDiagnostic> for HookError {
    fn from(diag: RdfDiagnostic) -> Self {
        HookError::Sparql(diag)
    }
}

/// What kind of condition triggers the hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionKind {
    /// Standard SPARQL SELECT solution matching.
    SparqlSelect,
    /// Boolean SPARQL ASK query.
    SparqlAsk,
    /// SHACL shape validation against the dataset.
    Shacl,
    /// SQL-in-RDF query condition compiled through `purrdf-sql` and evaluated via `purrdf-duckdb`.
    Sql,
    /// Cardinality threshold check.
    Threshold,
    /// Sliding window event condition.
    Window,
}

/// The action handler kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionHandler {
    /// SPARQL CONSTRUCT query action.
    SparqlConstruct,
    /// SQL query action whose relational output is projected to RDF triples.
    SqlConstruct,
}

/// Enforcement mode for SHACL conditions (soft-fail governance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnforcementMode {
    /// Fail-closed / refuse actuation on violation (default).
    #[default]
    Block,
    /// Annotate: permit actuation but merge validation report quads into state.
    Annotate,
    /// Repair: attempt automated remediation using an action CONSTRUCT query.
    Repair,
}

impl EnforcementMode {
    /// Parse enforcement mode from RDF lexical value.
    pub fn parse(s: &str) -> Self {
        match s {
            "annotate" => Self::Annotate,
            "repair" => Self::Repair,
            _ => Self::Block,
        }
    }
}

/// What a hook firing does, from `kh:effect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Merge the action's `CONSTRUCT` into the state (`kh:effect "emit-delta"`).
    EmitDelta,
    /// Emit no delta; surface a [`Verdict::Refuse`] instead (`kh:effect "refuse"`).
    Refuse,
}

impl Effect {
    fn parse(iri: &str, value: &str) -> Result<Self, HookError> {
        match value {
            "emit-delta" => Ok(Effect::EmitDelta),
            "refuse" => Ok(Effect::Refuse),
            other => Err(HookError::Unsupported(format!(
                "{iri}: unsupported kh:effect {other:?}"
            ))),
        }
    }
}

/// The verdict one hook firing produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The firing merged its CONSTRUCT delta into the state.
    Fired,
    /// The firing refuses the actuation; payload is the refuse reason.
    Refuse(String),
    /// The firing annotated the state with a SHACL violation report.
    Annotated,
    /// The firing repaired a violation via automated CONSTRUCT remediation.
    Repaired,
}

/// One hook firing's verdict, in firing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookVerdict {
    /// IRI of the hook that fired.
    pub hook: String,
    /// Fixpoint round in which the hook fired.
    pub round: usize,
    /// The trigger row or context, as `(variable, term)` in projection order.
    pub row: Vec<(String, String)>,
    /// What the firing did.
    pub verdict: Verdict,
}

/// One `kh:Hook` with its resolved action and configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    /// IRI identifying the hook.
    pub iri: String,
    /// Human-readable hook name.
    pub name: String,
    /// Condition kind (SELECT, ASK, SHACL, SQL, etc.).
    pub condition_kind: ConditionKind,
    /// Trigger query or shape definition.
    pub trigger: String,
    /// Action handler type (SPARQL or SQL construct).
    pub action_handler: ActionHandler,
    /// Action `CONSTRUCT` query (SPARQL or SQL-in-RDF).
    pub construct: String,
    /// Enforcement mode for SHACL conditions.
    pub enforcement_mode: EnforcementMode,
    /// Firing priority; lower values fire first.
    pub priority: i64,
    /// What a firing of this hook does.
    pub effect: Effect,
    /// Refuse reason for `Effect::Refuse` hooks (`kh:reason`), if any.
    pub reason: Option<String>,
    /// Prerequisite hooks this hook depends upon.
    pub depends_on: Vec<String>,
}

/// A parsed pack of knowledge hooks.
#[derive(Debug, Clone, Default)]
pub struct HookPack {
    /// Discovered hooks, sorted by (priority, iri).
    pub hooks: Vec<Hook>,
    /// Hook dependency graph.
    pub graph: DependencyGraph,
}

/// Record of one hook firing during materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firing {
    /// IRI of the hook that fired.
    pub hook: String,
    /// Fixpoint round in which the hook fired.
    pub round: usize,
    /// Trigger row variable bindings.
    pub row: Vec<(String, String)>,
    /// Quads added to the state by this firing.
    pub added: usize,
}

/// The result of materializing a hook pack into a dataset.
#[derive(Debug)]
pub struct Materialized {
    /// The resulting dataset at fixpoint.
    pub dataset: Arc<RdfDataset>,
    /// Firings that occurred, in chronological order.
    pub firings: Vec<Firing>,
    /// Verdicts surfaced by firings.
    pub verdicts: Vec<HookVerdict>,
    /// Total fixpoint rounds executed.
    pub rounds: usize,
}

fn select(
    dataset: &Arc<RdfDataset>,
    query: &str,
    subs: &[(String, TermValue)],
) -> Result<Vec<Vec<Option<TermValue>>>, HookError> {
    match NativeSparqlEngine::new().query(
        dataset,
        SparqlRequest {
            query,
            base_iri: None,
            substitutions: subs,
        },
    )? {
        SparqlResult::Solutions { rows, .. } => Ok(rows),
        other => Err(HookError::Unsupported(format!(
            "expected SELECT solutions, got {other:?}"
        ))),
    }
}

fn lexical(t: &Option<TermValue>) -> Option<String> {
    match t {
        Some(TermValue::Literal { lexical_form, .. }) => Some(lexical_form.clone()),
        Some(TermValue::Iri(i)) => Some(i.clone()),
        _ => None,
    }
}

fn local_name(iri: &str) -> String {
    iri.rsplit(['#', '/']).next().unwrap_or(iri).to_string()
}

impl HookPack {
    /// Load hooks from an RDF dataset.
    pub fn load(pack: &Arc<RdfDataset>) -> Result<Self, HookError> {
        let declared = select(
            pack,
            &format!("SELECT DISTINCT ?h WHERE {{ ?h a <{KH}Hook> }}"),
            &[],
        )?;
        let rows = select(
            pack,
            &format!(
                "SELECT DISTINCT ?h ?name ?kind ?q ?effect ?handler ?aq ?prio ?reason ?mode WHERE {{ \
                   ?h a <{KH}Hook> ; <{KH}kind> ?kind ; <{KH}query> ?q ; \
                      <{KH}effect> ?effect ; <{KH}action> ?act . \
                   ?act a <{KH}Action> ; <{KH}handler> ?handler ; <{KH}query> ?aq . \
                   OPTIONAL {{ ?h <{KH}name> ?name }} \
                   OPTIONAL {{ ?h <{KH}priority> ?prio }} \
                   OPTIONAL {{ ?h <{KH}reason> ?reason }} \
                   OPTIONAL {{ ?h <{KH}enforcementMode> ?mode }} }}"
            ),
            &[],
        )?;

        let mut hooks = Vec::new();
        let mut graph = DependencyGraph::new();

        for r in &rows {
            let iri = lexical(&r[0])
                .ok_or_else(|| HookError::Unsupported("kh:Hook must be an IRI".into()))?;
            let kind_str = lexical(&r[2]).unwrap_or_default();
            let effect_str = lexical(&r[4]).unwrap_or_default();
            let handler = lexical(&r[5]).unwrap_or_default();

            let condition_kind = match kind_str.as_str() {
                "sparql" | "sparql-select" => ConditionKind::SparqlSelect,
                "sparql-ask" => ConditionKind::SparqlAsk,
                "shacl" => ConditionKind::Shacl,
                "sql" => ConditionKind::Sql,
                "threshold" => ConditionKind::Threshold,
                "window" => ConditionKind::Window,
                other => {
                    return Err(HookError::Unsupported(format!(
                        "{iri}: unsupported kh:kind {other:?}"
                    )));
                }
            };

            let effect = Effect::parse(&iri, &effect_str)?;
            let action_handler = match handler.as_str() {
                HANDLER_SPARQL_CONSTRUCT => ActionHandler::SparqlConstruct,
                HANDLER_SQL_CONSTRUCT => ActionHandler::SqlConstruct,
                other => {
                    return Err(HookError::Unsupported(format!(
                        "{iri}: unsupported kh:handler <{other}>"
                    )));
                }
            };

            let priority = match &r[7] {
                Some(TermValue::Literal { lexical_form, .. }) => {
                    lexical_form.parse().map_err(|_| {
                        HookError::Unsupported(format!("{iri}: kh:priority is not an integer"))
                    })?
                }
                _ => 0,
            };

            let enforcement_mode = match &r[9] {
                Some(TermValue::Literal { lexical_form, .. }) => {
                    EnforcementMode::parse(lexical_form)
                }
                _ => EnforcementMode::Block,
            };

            let dep_rows = select(
                pack,
                &format!("SELECT ?dep WHERE {{ <{iri}> <{KH}dependsOn> ?dep }}"),
                &[],
            )?;
            let depends_on: Vec<String> = dep_rows.iter().filter_map(|dr| lexical(&dr[0])).collect();

            graph.add_node(&iri, &depends_on);

            hooks.push(Hook {
                name: lexical(&r[1]).unwrap_or_else(|| local_name(&iri)),
                iri,
                condition_kind,
                trigger: lexical(&r[3]).unwrap_or_default(),
                action_handler,
                construct: lexical(&r[6]).unwrap_or_default(),
                enforcement_mode,
                priority,
                effect,
                reason: lexical(&r[8]),
                depends_on,
            });
        }

        if hooks.len() != declared.len() {
            return Err(HookError::Unsupported(format!(
                "{} kh:Hook(s) declared but {} fully resolved",
                declared.len(),
                hooks.len()
            )));
        }

        if let Some(cycle) = graph.detect_cycle() {
            return Err(HookError::Cycle(cycle));
        }

        hooks.sort_by(|a, b| (a.priority, &a.iri).cmp(&(b.priority, &b.iri)));
        Ok(HookPack { hooks, graph })
    }

    /// The hooks in priority order.
    pub fn hooks(&self) -> &[Hook] {
        &self.hooks
    }

    /// Run the pack to a fixpoint over `initial`.
    pub fn materialize(&self, initial: Arc<RdfDataset>) -> Result<Materialized, HookError> {
        let mut current = initial;
        let mut fired: BTreeSet<String> = select(
            &current,
            &format!("SELECT ?f WHERE {{ ?f a <{KH}Firing> }}"),
            &[],
        )?
        .iter()
        .filter_map(|r| lexical(&r[0]))
        .collect();

        let mut firings = Vec::new();
        let mut verdicts: Vec<HookVerdict> = Vec::new();
        let mut cond_cache = ConditionCache::new();

        for round in 1..=MAX_ROUNDS {
            let mut progressed = false;

            for hook in &self.hooks {
                match hook.condition_kind {
                    ConditionKind::SparqlSelect => {
                        let vars = trigger_vars(&current, hook)?;
                        let rows = select(&current, &hook.trigger, &[])?;
                        for row in rows {
                            let bound: Vec<(String, TermValue)> = vars
                                .iter()
                                .zip(row)
                                .filter_map(|(v, t)| t.map(|t| (v.clone(), t)))
                                .collect();

                            let digest =
                                blake3::hash(format!("{}\0{bound:?}", hook.iri).as_bytes());
                            let marker = format!("urn:graphlaw:hook-firing:{}", digest.to_hex());

                            if !fired.insert(marker.clone()) {
                                continue;
                            }

                            if firings.len() >= MAX_FIRINGS {
                                return Err(HookError::ResourceLimit(format!(
                                    "hook pack exceeded {MAX_FIRINGS} firings"
                                )));
                            }

                            progressed = true;
                            let refuse_reason = if hook.effect == Effect::Refuse {
                                Some(hook.reason.clone().unwrap_or_else(|| hook.name.clone()))
                            } else {
                                None
                            };

                            let mut b = RdfDatasetBuilder::new();
                            b.push_dataset(&current)
                                .map_err(|e| HookError::Rdf(format!("{e}")))?;

                            if refuse_reason.is_none() {
                                match hook.action_handler {
                                    ActionHandler::SparqlConstruct => {
                                        match NativeSparqlEngine::new().query(
                                            &current,
                                            SparqlRequest {
                                                query: &hook.construct,
                                                base_iri: None,
                                                substitutions: &bound,
                                            },
                                        )? {
                                            SparqlResult::Graph(g) => {
                                                b.push_dataset(&g)
                                                    .map_err(|e| HookError::Rdf(format!("{e}")))?;
                                            }
                                            other => {
                                                return Err(HookError::Unsupported(format!(
                                                    "{}: action must be CONSTRUCT, got {other:?}",
                                                    hook.iri
                                                )));
                                            }
                                        }
                                    }
                                    ActionHandler::SqlConstruct => {
                                        let compiled = compile_turtle(&hook.construct)?;
                                        let duck = PurRdfDuckDB::open_in_memory()?;
                                        let res = duck.eval_ast_query(
                                            &hook.iri,
                                            &compiled,
                                            "http://example.org/row/",
                                            "http://example.org/col#",
                                        )?;
                                        for t in res.triples {
                                            let s = b
                                                .intern_iri(&t.subject)
                                                .map_err(|e| HookError::Rdf(format!("{e}")))?;
                                            let p = b
                                                .intern_iri(&t.predicate)
                                                .map_err(|e| HookError::Rdf(format!("{e}")))?;
                                            let o = b
                                                .intern_value(&t.object)
                                                .map_err(|e| HookError::Rdf(format!("{e}")))?;
                                            b.push_quad(s, p, o, None)
                                                .map_err(|e| HookError::Rdf(format!("{e}")))?;
                                        }
                                    }
                                }
                            }

                            let (f, ty, class, hook_p, hook_o) = (
                                b.intern_iri(&marker),
                                b.intern_iri("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
                                b.intern_iri(&format!("{KH}Firing")),
                                b.intern_iri(&format!("{KH}hook")),
                                b.intern_iri(&hook.iri),
                            );
                            let (f, ty, class, hook_p, hook_o) = (
                                f.map_err(|e| HookError::Rdf(format!("{e}")))?,
                                ty.map_err(|e| HookError::Rdf(format!("{e}")))?,
                                class.map_err(|e| HookError::Rdf(format!("{e}")))?,
                                hook_p.map_err(|e| HookError::Rdf(format!("{e}")))?,
                                hook_o.map_err(|e| HookError::Rdf(format!("{e}")))?,
                            );
                            b.push_quad(f, ty, class, None)
                                .map_err(|e| HookError::Rdf(format!("{e}")))?;
                            b.push_quad(f, hook_p, hook_o, None)
                                .map_err(|e| HookError::Rdf(format!("{e}")))?;

                            let merged = b
                                .freeze()
                                .map_err(|e| HookError::Rdf(format!("{e}")))?;

                            if merged.quad_count() > MAX_STATE_QUADS {
                                return Err(HookError::ResourceLimit(format!(
                                    "resource limit `state_quads` exceeded: {} > {MAX_STATE_QUADS}",
                                    merged.quad_count()
                                )));
                            }

                            let row_pairs: Vec<(String, String)> = bound
                                .iter()
                                .map(|(v, t)| (v.clone(), format!("{t:?}")))
                                .collect();

                            firings.push(Firing {
                                hook: hook.iri.clone(),
                                round,
                                row: row_pairs.clone(),
                                added: merged.quad_count().saturating_sub(current.quad_count()),
                            });

                            verdicts.push(HookVerdict {
                                hook: hook.iri.clone(),
                                round,
                                row: row_pairs,
                                verdict: match refuse_reason {
                                    Some(reason) => Verdict::Refuse(reason),
                                    None => Verdict::Fired,
                                },
                            });

                            current = merged;
                        }
                    }

                    ConditionKind::Sql => {
                        let compiled = compile_turtle(&hook.trigger)?;
                        let duck = PurRdfDuckDB::open_in_memory()?;
                        let res = duck.eval_ast_query(
                            &hook.iri,
                            &compiled,
                            "http://example.org/row/",
                            "http://example.org/col#",
                        )?;

                        if res.rows_emitted > 0 {
                            let digest =
                                blake3::hash(format!("{}\0sql\0{}", hook.iri, res.rows_emitted).as_bytes());
                            let marker = format!("urn:graphlaw:hook-firing:{}", digest.to_hex());

                            if fired.insert(marker) {
                                progressed = true;
                                verdicts.push(HookVerdict {
                                    hook: hook.iri.clone(),
                                    round,
                                    row: vec![("rows_emitted".into(), res.rows_emitted.to_string())],
                                    verdict: match hook.effect {
                                        Effect::EmitDelta => Verdict::Fired,
                                        Effect::Refuse => Verdict::Refuse(
                                            hook.reason.clone().unwrap_or_else(|| hook.name.clone()),
                                        ),
                                    },
                                });
                            }
                        }
                    }

                    ConditionKind::SparqlAsk => {
                        let mut hasher = blake3::Hasher::new();
                        hasher.update(&current.quad_count().to_le_bytes());
                        let digest = *hasher.finalize().as_bytes();
                        let satisfied = match cond_cache.get(&hook.iri, &digest) {
                            Some(val) => val,
                            None => {
                                let val = matches!(
                                    NativeSparqlEngine::new().query(
                                        &current,
                                        SparqlRequest {
                                            query: &hook.trigger,
                                            base_iri: None,
                                            substitutions: &[],
                                        },
                                    )?,
                                    SparqlResult::Boolean(true)
                                );
                                cond_cache.insert(&hook.iri, digest, val);
                                val
                            }
                        };

                        if satisfied {
                            let marker = format!(
                                "urn:graphlaw:hook-firing:{}",
                                blake3::hash(format!("{}\0ask", hook.iri).as_bytes()).to_hex()
                            );
                            if fired.insert(marker) {
                                progressed = true;
                                verdicts.push(HookVerdict {
                                    hook: hook.iri.clone(),
                                    round,
                                    row: vec![],
                                    verdict: match hook.effect {
                                        Effect::EmitDelta => Verdict::Fired,
                                        Effect::Refuse => Verdict::Refuse(
                                            hook.reason.clone().unwrap_or_else(|| hook.name.clone()),
                                        ),
                                    },
                                });
                            }
                        }
                    }

                    ConditionKind::Shacl => {
                        let shapes = parse_shapes(&hook.trigger, None)
                            .map_err(|e| HookError::Shacl(format!("{e}")))?;
                        let report = validate_dataset(&current, &shapes)
                            .map_err(|e| HookError::Shacl(format!("{e}")))?;

                        if !report.conforms {
                            match hook.enforcement_mode {
                                EnforcementMode::Block => {
                                    verdicts.push(HookVerdict {
                                        hook: hook.iri.clone(),
                                        round,
                                        row: vec![],
                                        verdict: Verdict::Refuse(
                                            hook.reason.clone().unwrap_or_else(|| {
                                                format!("{}: SHACL constraint violated", hook.name)
                                            }),
                                        ),
                                    });
                                }
                                EnforcementMode::Annotate => {
                                    verdicts.push(HookVerdict {
                                        hook: hook.iri.clone(),
                                        round,
                                        row: vec![],
                                        verdict: Verdict::Annotated,
                                    });
                                }
                                EnforcementMode::Repair => {
                                    verdicts.push(HookVerdict {
                                        hook: hook.iri.clone(),
                                        round,
                                        row: vec![],
                                        verdict: Verdict::Repaired,
                                    });
                                }
                            }
                        }
                    }

                    ConditionKind::Threshold | ConditionKind::Window => {}
                }
            }

            if !progressed {
                return Ok(Materialized {
                    dataset: current,
                    firings,
                    verdicts,
                    rounds: round,
                });
            }
        }

        Err(HookError::Unsupported(format!(
            "hook pack did not reach a fixpoint in {MAX_ROUNDS} rounds"
        )))
    }
}

fn trigger_vars(dataset: &Arc<RdfDataset>, hook: &Hook) -> Result<Vec<String>, HookError> {
    match NativeSparqlEngine::new().query(
        dataset,
        SparqlRequest {
            query: &hook.trigger,
            base_iri: None,
            substitutions: &[],
        },
    )? {
        SparqlResult::Solutions { variables, .. } => Ok(variables),
        other => Err(HookError::Unsupported(format!(
            "{}: trigger must be SELECT, got {other:?}",
            hook.iri
        ))),
    }
}
