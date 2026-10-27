//! Proof checking: `docs/proof-checking.md`'s reference implementation.
//!
//! The validity conditions are checked *here*, once, over an abstract
//! view of a proof document (`Document`). A reader presents N3 proof syntax
//! through that view and re-performs the document's own inferences.
//!
//! Nothing in this module reasons. It never searches for a derivation, only
//! re-performs the ones the document recorded — which is what keeps a
//! checker small enough to be worth trusting (specification §2).

use std::collections::BTreeMap;

/// The one reason a step carries. `Kind` is what the generic checker needs;
/// the locator (which rule, which document) stays with the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The step applies a written rule.
    Rule,
    /// The step asserts a premise the source gives outright.
    Fact,
    /// The step re-decides a built-in predicate.
    Builtin,
    /// The step's justification itself is missing.
    Absent,
    /// The step collects a previously derived statement.
    Collected,
    /// The step is recorded as unproven.
    Unproven,
}

impl Kind {
    /// The lower-case label the report uses for this kind.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Fact => "fact",
            Kind::Builtin => "builtin",
            Kind::Absent => "absent",
            Kind::Collected => "collected",
            Kind::Unproven => "unproven",
        }
    }
}

/// The validity conditions of specification §4, in report order.
pub const CONDITIONS: [(&str, &str); 9] = [
    ("C1", "resolution"),
    ("C2", "well-foundedness"),
    ("C3", "justification"),
    ("C4", "coverage"),
    ("C5", "re-decision"),
    ("C6", "boundary consistency"),
    ("C7", "relevance"),
    ("C8", "witness integrity"),
    ("C9", "query isolation"),
];

fn condition_index(condition: &str) -> usize {
    CONDITIONS
        .iter()
        .position(|(id, _)| *id == condition)
        .expect("a known condition")
}

/// Where a premise or carried-rule dependency resolves (§4 C4).
#[derive(Debug, Clone)]
pub enum Resolution {
    /// To the conclusion of this step, of which it is an instance.
    Step(usize),
    /// To a statement the source gives outright.
    Given,
    /// Nowhere; the string says what could not be resolved.
    Unresolved(String),
}

/// What checking one step established.
#[derive(Debug)]
pub enum Checked {
    /// The recorded inference was re-performed and matched (§5).
    Verified,
    /// Recorded as an obligation rather than re-established (§6); the
    /// string says which obligation.
    Trusted(&'static str),
}

/// A step recorded as trusted rather than re-performed (§6).
#[derive(Debug, Clone)]
pub struct Obligation {
    /// The obligation's class, e.g. which trusted builtin family.
    pub kind: &'static str,
    /// The step conclusion the obligation covers.
    pub conclusion: String,
}

/// One failed validity condition, with the step or claim it concerns.
#[derive(Debug, Clone)]
pub struct Failure {
    /// The validity condition that failed: `"C1"`..`"C9"`.
    pub condition: &'static str,
    /// The step or claim conclusion the failure concerns.
    pub conclusion: String,
    /// What about the conclusion failed the condition.
    pub detail: String,
}

/// The full outcome of checking one proof document.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Number of steps examined.
    pub steps: usize,
    /// Steps whose recorded inference was re-performed and matched (§5).
    pub verified: usize,
    /// Steps recorded as obligations rather than re-established (§6).
    pub obligations: Vec<Obligation>,
    /// Each validity-condition failure.
    pub failures: Vec<Failure>,
    /// Step counts by [`Kind::label`].
    pub counts: BTreeMap<&'static str, usize>,
    /// How many items each condition examined, in `CONDITIONS` order.
    pub checked: [usize; CONDITIONS.len()],
}

impl Report {
    /// True when no validity condition failed.
    pub fn valid(&self) -> bool {
        self.failures.is_empty()
    }

    /// Each condition with how many items it examined and how many
    /// failures it reported, in `CONDITIONS` order.
    pub fn conditions(&self) -> Vec<(&'static str, &'static str, usize, usize)> {
        CONDITIONS
            .iter()
            .enumerate()
            .map(|(position, (id, name))| {
                let failed = self
                    .failures
                    .iter()
                    .filter(|failure| failure.condition == *id)
                    .count();
                (*id, *name, self.checked[position], failed)
            })
            .collect()
    }

    fn fail(&mut self, condition: &'static str, conclusion: String, detail: String) {
        self.failures.push(Failure {
            condition,
            conclusion,
            detail,
        });
    }

    /// The verdict line the specification's §9 requires.
    pub fn verdict(&self) -> String {
        let steps = format!(
            "{} steps, {} verified, {} trusted",
            self.steps,
            self.verified,
            self.obligations.len()
        );
        if !self.valid() {
            return format!("invalid: {} failure(s); {}", self.failures.len(), steps);
        }
        if self.obligations.is_empty() {
            format!("checked: {}", steps)
        } else {
            format!("checked with obligations: {}", steps)
        }
    }
}

/// A proof document, seen the way the checker needs to see it. A reader
/// implements this over its own syntax.
///
/// Resolution and re-performing a step belong to the reader rather than to
/// the checker, because deciding that one statement is an instance of
/// another is a matter of that formalism's terms. Everything else — which
/// condition each finding belongs to, cycles, relevance, and the
/// bookkeeping — is here and shared.
pub trait Document {
    /// How many proof steps the document records.
    fn step_count(&self) -> usize;

    /// Step `index`'s kind of justification.
    fn kind(&self, index: usize) -> Kind;

    /// Step `index`'s conclusion, for reporting.
    fn describe(&self, index: usize) -> String;

    /// Where the step's premises and carried-rule provenance resolve.
    fn dependencies(&self, index: usize) -> Vec<Resolution>;

    /// The statements the document asserts were derived, for reporting.
    fn claims(&self) -> Vec<String>;

    /// Where claim `index` resolves.
    fn claim_resolution(&self, index: usize) -> Resolution;

    /// Additional evidence selecting a claim for output, such as the
    /// other conditions of a query that selects an already proved fact.
    fn claim_dependencies(&self, _index: usize) -> Vec<Resolution> {
        Vec::new()
    }

    /// Re-perform step `index`'s recorded inference (§5), or classify it as
    /// an obligation (§6), under the condition that governs its kind of
    /// justification: C1 for rules and facts, C5 for built-ins, C3 when the
    /// justification itself is missing.
    fn check_step(&self, index: usize) -> StepCheck;

    /// (C6) Look for evidence inside the document that contradicts what
    /// its obligations assume: how many boundaries were examined, and each
    /// contradiction as (conclusion, detail).
    fn boundary(&self) -> (usize, Vec<(String, String)>) {
        (0, Vec::new())
    }

    /// (C7) Whether claim `index` answers what the source asked for.
    fn claim_relevance(&self, _index: usize) -> Result<(), String> {
        Ok(())
    }

    /// (C8) Existential witnesses keep their identity across recorded
    /// applications and are fresh for the existential that introduces them.
    fn witnesses(&self) -> (usize, Vec<(String, String)>) {
        (0, Vec::new())
    }

    /// (C9) Query applications justify output without adding premises or
    /// rules to the inference graph. Query outputs need query support.
    fn query_isolation(&self) -> (usize, Vec<(String, String)>) {
        (0, Vec::new())
    }
}

/// What checking one step's justification found, and under which
/// condition.
#[derive(Debug)]
pub struct StepCheck {
    /// The validity condition the justification was checked under.
    pub condition: &'static str,
    /// What the check established, or why it failed.
    pub outcome: Result<Checked, String>,
}

/// Check `document` against the source program it was read with, applying
/// the conditions of specification §4.
pub fn check(document: &dyn Document) -> Report {
    let mut report = Report {
        steps: document.step_count(),
        ..Report::default()
    };

    for index in 0..document.step_count() {
        *report
            .counts
            .entry(document.kind(index).label())
            .or_insert(0) += 1;
    }

    // (C4) every premise and carried rule resolves; collect the step-to-step
    // edges C2 and C7 need while we are here.
    let mut edges: Vec<Vec<usize>> = Vec::with_capacity(document.step_count());
    for index in 0..document.step_count() {
        let mut dependencies = Vec::new();
        for resolution in document.dependencies(index) {
            report.checked[condition_index("C4")] += 1;
            match resolution {
                Resolution::Step(target) => dependencies.push(target),
                Resolution::Given => {}
                Resolution::Unresolved(detail) => report.fail("C4", document.describe(index),
                    format!("uses {}, which is neither an instance of a step's conclusion nor given by the source", detail)),
            }
        }
        edges.push(dependencies);
    }

    // (C2) the step-to-step use relation is well founded.
    report.checked[condition_index("C2")] = document.step_count();
    check_well_founded(document, &edges, &mut report);

    // (C1, C3, C5) every step's justification holds. Every step has its
    // kind of justification examined (C3); rules and facts are
    // re-performed (C1), built-ins re-decided (C5).
    report.checked[condition_index("C3")] = document.step_count();
    for index in 0..document.step_count() {
        let StepCheck { condition, outcome } = document.check_step(index);
        if condition != "C3" {
            report.checked[condition_index(condition)] += 1;
        }
        match outcome {
            Ok(Checked::Verified) => report.verified += 1,
            Ok(Checked::Trusted(kind)) => report.obligations.push(Obligation {
                kind,
                conclusion: document.describe(index),
            }),
            Err(detail) => report.fail(condition, document.describe(index), detail),
        }
    }

    // (C4) every claim is accounted for, and (C3) claimed once.
    let claims = document.claims();
    let mut claimed = std::collections::BTreeSet::new();
    let mut roots = Vec::new();
    for (index, claim) in claims.iter().enumerate() {
        report.checked[condition_index("C4")] += 1;
        if !claimed.insert(claim.clone()) {
            report.fail("C3", claim.clone(), "is claimed more than once".to_string());
        }
        match document.claim_resolution(index) {
            Resolution::Step(target) => roots.push(target),
            Resolution::Given => {}
            Resolution::Unresolved(_) => report.fail(
                "C4",
                claim.clone(),
                "claimed, but no step concludes it and the source does not give it".to_string(),
            ),
        }
        for dependency in document.claim_dependencies(index) {
            report.checked[condition_index("C4")] += 1;
            match dependency {
                Resolution::Step(target) => roots.push(target),
                Resolution::Given => {}
                Resolution::Unresolved(detail) => report.fail("C4", claim.clone(), detail),
            }
        }
    }

    // (C6) nothing the document records contradicts what it assumes.
    let (examined, contradictions) = document.boundary();
    report.checked[condition_index("C6")] = examined;
    for (conclusion, detail) in contradictions {
        report.fail("C6", conclusion, detail);
    }

    // (C7) every claim answers the source, and every step serves a claim.
    report.checked[condition_index("C7")] = claims.len() + document.step_count();
    for (index, claim) in claims.iter().enumerate() {
        if let Err(detail) = document.claim_relevance(index) {
            report.fail("C7", claim.clone(), detail);
        }
    }
    let mut serves = vec![false; document.step_count()];
    while let Some(index) = roots.pop() {
        if !std::mem::replace(&mut serves[index], true) {
            roots.extend(edges[index].iter().copied());
        }
    }
    for (index, serving) in serves.iter().enumerate() {
        if !serving {
            report.fail(
                "C7",
                document.describe(index),
                "serves no claim: no claim depends on it".to_string(),
            );
        }
    }

    for (condition, (examined, failures)) in [
        ("C8", document.witnesses()),
        ("C9", document.query_isolation()),
    ] {
        report.checked[condition_index(condition)] = examined;
        for (conclusion, detail) in failures {
            report.fail(condition, conclusion, detail);
        }
    }

    report
        .failures
        .sort_by_key(|failure| condition_index(failure.condition));
    report
}

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    Open,
    Done,
}

/// Depth-first search for a cycle in the step-to-step `uses` relation,
/// iterative because a long derivation chain (`deep-taxonomy-100000`) is
/// exactly what this gets pointed at.
fn check_well_founded(document: &dyn Document, edges: &[Vec<usize>], report: &mut Report) {
    let mut marks: Vec<Option<Mark>> = vec![None; edges.len()];
    for root in 0..edges.len() {
        if marks[root].is_some() {
            continue;
        }
        marks[root] = Some(Mark::Open);
        // Each frame is a step and how many of its dependencies have been
        // taken. Iterative, because a long derivation chain
        // (`deep-taxonomy-100000`) is exactly what this gets pointed at.
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some((index, cursor)) = stack.last_mut() {
            let dependencies = &edges[*index];
            if *cursor == dependencies.len() {
                marks[*index] = Some(Mark::Done);
                stack.pop();
                continue;
            }
            let next = dependencies[*cursor];
            *cursor += 1;
            match marks[next] {
                Some(Mark::Done) => {}
                Some(Mark::Open) => {
                    report.fail(
                        "C2",
                        document.describe(next),
                        "is used, directly or indirectly, by its own derivation".to_string(),
                    );
                    marks[next] = Some(Mark::Done);
                }
                None => {
                    marks[next] = Some(Mark::Open);
                    stack.push((next, 0));
                }
            }
        }
    }
}
