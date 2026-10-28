use crate::ast::*;
use crate::reasoner::FiredFuse;
use purrdf_lex::json_escape::{JsonEscapes, push_body};
use std::collections::{BTreeMap, BTreeSet};

/// Renders reasoner output triples as N3: joined `log:outputString` values when
/// present, otherwise the full N3 serialization of the triples.
pub fn result_to_string(prefixes: &BTreeMap<String, String>, triples: &[Triple]) -> String {
    let output_strings: Vec<String> = triples
        .iter()
        .filter_map(|t| match (&t.p, &t.o) {
            (Term::Iri(p), Term::Literal(l)) if p.as_str() == LOG_OUTPUT_STRING => {
                Some(l.value.to_string())
            }
            _ => None,
        })
        .collect();

    if !output_strings.is_empty() {
        return output_strings.join("");
    }
    triples_to_n3(prefixes, triples)
}

/// Renders reasoner output triples as TriG (RDF-compat surface): joined
/// `log:outputString` values when present, otherwise the TriG serialization.
pub fn rdf_result_to_string(prefixes: &BTreeMap<String, String>, triples: &[Triple]) -> String {
    let output_strings: Vec<String> = triples
        .iter()
        .filter_map(|t| match (&t.p, &t.o) {
            (Term::Iri(p), Term::Literal(l)) if p.as_str() == LOG_OUTPUT_STRING => {
                Some(l.value.to_string())
            }
            _ => None,
        })
        .collect();

    if !output_strings.is_empty() {
        return output_strings.join("");
    }
    triples_to_trig(prefixes, triples)
}

/// Renders one triple as a single N3 statement (implications as rule syntax).
pub fn triple_to_n3(prefixes: &BTreeMap<String, String>, t: &Triple) -> String {
    if is_implication_triple(t) {
        implication_to_n3(t, prefixes).trim_end().to_string()
    } else {
        format!(
            "{} {} {} .",
            term_to_n3(&t.s, prefixes, Position::Subject),
            term_to_n3(&t.p, prefixes, Position::Predicate),
            term_to_n3(&t.o, prefixes, Position::Object),
        )
    }
}

/// Renders one term in the N3 object position (blank nodes as `[...]` where apt).
pub fn term_to_n3_object(term: &Term, prefixes: &BTreeMap<String, String>) -> String {
    term_to_n3(term, prefixes, Position::Object)
}

/// Renders one term in the N3 predicate position (`a` for `rdf:type`, `~` reversal).
pub fn term_to_n3_predicate(term: &Term, prefixes: &BTreeMap<String, String>) -> String {
    term_to_n3(term, prefixes, Position::Predicate)
}

/// Serializes triples as a full N3 document with `@prefix` declarations.
pub fn triples_to_n3(prefixes: &BTreeMap<String, String>, triples: &[Triple]) -> String {
    if triples.is_empty() {
        return String::new();
    }
    let used = used_prefixes(prefixes, triples);
    let mut out = String::new();

    if used.contains("") {
        if let Some(base) = prefixes.get("") {
            out.push_str(&format!("@prefix : <{base}> .\n"));
        }
    }
    for p in &used {
        if p.is_empty() {
            continue;
        }
        if let Some(base) = prefixes.get(p) {
            out.push_str(&format!("@prefix {p}: <{base}> .\n"));
        }
    }
    if !used.is_empty() {
        out.push('\n');
    }

    let (declarations, rendered_triples) =
        explicit_graph_for_printing(&Graph::from(triples.to_vec()));
    for declaration in declarations {
        out.push_str(&declaration);
        out.push('\n');
    }
    let mut prev_multiline = false;
    let mut first = true;
    for t in &rendered_triples {
        let rendered = if is_implication_triple(t) {
            implication_to_n3(t, prefixes)
        } else {
            format!(
                "{} {} {} .\n",
                term_to_n3(&t.s, prefixes, Position::Subject),
                term_to_n3(&t.p, prefixes, Position::Predicate),
                term_to_n3(&t.o, prefixes, Position::Object),
            )
        };
        // A triple whose subject or object is itself a quoted formula (an
        // implication, a proof step, ...) renders as several
        // lines; give it its own paragraph rather than crowding it
        // against a neighboring one-line triple or another such block.
        let this_multiline = rendered.trim_end().contains('\n');
        if !first && (prev_multiline || this_multiline) {
            out.push('\n');
        }
        out.push_str(&rendered);
        prev_multiline = this_multiline;
        first = false;
    }
    out
}

/// Serializes triples as a TriG document (named graphs rendered as `GRAPH` blocks).
pub fn triples_to_trig(prefixes: &BTreeMap<String, String>, triples: &[Triple]) -> String {
    if triples.is_empty() {
        return String::new();
    }
    let used = used_prefixes_for_trig(prefixes, triples);
    let mut out = String::new();

    if used.contains("") {
        if let Some(base) = prefixes.get("") {
            out.push_str(&format!("@prefix : <{base}> .\n"));
        }
    }
    for p in &used {
        if p.is_empty() {
            continue;
        }
        if let Some(base) = prefixes.get(p) {
            out.push_str(&format!("@prefix {p}: <{base}> .\n"));
        }
    }
    if !used.is_empty() {
        out.push('\n');
    }

    let mut prev_multiline = false;
    let mut first = true;
    for t in triples {
        let (rendered, this_multiline) = if let Some((graph, graph_triples)) = named_graph_fact(t) {
            let mut block = format!("{} {{\n", term_to_n3(graph, prefixes, Position::Subject),);
            for inner in graph_triples {
                block.push_str(&format!(
                    "    {} {} {} .\n",
                    term_to_n3(&inner.s, prefixes, Position::Subject),
                    term_to_n3(&inner.p, prefixes, Position::Predicate),
                    term_to_n3(&inner.o, prefixes, Position::Object),
                ));
            }
            block.push_str("}\n");
            (block, true)
        } else {
            let line = format!(
                "{} {} {} .\n",
                term_to_n3(&t.s, prefixes, Position::Subject),
                term_to_n3(&t.p, prefixes, Position::Predicate),
                term_to_n3(&t.o, prefixes, Position::Object),
            );
            let multiline = line.trim_end().contains('\n');
            (line, multiline)
        };
        // A named-graph block or a quoted-formula-valued triple renders as
        // several lines; give it its own paragraph rather than crowding it
        // against a neighboring one-line triple or another such block.
        if !first && (prev_multiline || this_multiline) {
            out.push('\n');
        }
        out.push_str(&rendered);
        prev_multiline = this_multiline;
        first = false;
    }
    out
}

#[derive(Clone, Copy)]
enum Position {
    Subject,
    Predicate,
    Object,
}

fn term_to_n3(term: &Term, prefixes: &BTreeMap<String, String>, pos: Position) -> String {
    match term {
        Term::Iri(iri) if matches!(pos, Position::Predicate) && iri == RDF_TYPE => "a".to_string(),
        Term::Iri(iri) => compact_iri(iri, prefixes).unwrap_or_else(|| format!("<{iri}>")),
        Term::Var(name) => format!("?{name}"),
        Term::Blank(name) => format!("_:{}", sanitize_blank(name)),
        Term::Literal(lit) => literal_to_n3(lit, prefixes),
        Term::List(items) => {
            let rendered = items
                .iter()
                .map(|item| term_to_n3(item, prefixes, Position::Object))
                .collect::<Vec<_>>()
                .join(" ");
            format!("({rendered})")
        }
        Term::Formula(triples) if triples.is_empty() => "{}".to_string(),
        Term::Formula(triples) => formula_to_n3(triples, prefixes, 0),
    }
}

fn is_implication_triple(t: &Triple) -> bool {
    match (&t.s, &t.p, &t.o) {
        (Term::Formula(_), Term::Iri(p), Term::Formula(_)) => {
            p == LOG_IMPLIES || p == LOG_IMPLIED_BY
        }
        // An inference fuse concludes `false`, and reads back as one.
        (Term::Formula(_), Term::Iri(p), o) => {
            p == LOG_IMPLIES && crate::reasoner::is_boolean_false(o)
        }
        _ => false,
    }
}

fn implication_to_n3(t: &Triple, prefixes: &BTreeMap<String, String>) -> String {
    match (&t.s, &t.o) {
        (Term::Formula(lhs), o) if crate::reasoner::is_boolean_false(o) => {
            format!("{} => false .\n", formula_to_n3(lhs, prefixes, 0))
        }
        (Term::Formula(lhs), Term::Formula(rhs)) => {
            let op = match &t.p {
                Term::Iri(p) if p == LOG_IMPLIED_BY => "<=",
                _ => "=>",
            };
            format!(
                "{} {} {} .\n",
                formula_to_n3(lhs, prefixes, 0),
                op,
                formula_to_n3(rhs, prefixes, 0)
            )
        }
        _ => format!(
            "{} {} {} .\n",
            term_to_n3(&t.s, prefixes, Position::Subject),
            term_to_n3(&t.p, prefixes, Position::Predicate),
            term_to_n3(&t.o, prefixes, Position::Object),
        ),
    }
}

/// What an inference fuse prints when it fires: the rule that forbade the
/// situation, and, when the match made it concrete, the situation itself.
/// Every line is a comment, so the report can be read back as the empty
/// graph it describes -- nothing was derived, the run stopped.
pub fn fuse_report(prefixes: &BTreeMap<String, String>, fuse: &FiredFuse) -> String {
    let mut out = String::from("# Inference fuse triggered.\n");
    let schematic = fuse_rule_to_n3(&fuse.rule.premise, prefixes);
    out.push_str("# Fired rule:\n");
    for line in schematic.lines() {
        out.push_str("#   ");
        out.push_str(line);
        out.push('\n');
    }
    let instance = fuse_rule_to_n3(&fuse.instance, prefixes);
    if instance != schematic {
        out.push_str("# Matched instance:\n");
        for line in instance.lines() {
            out.push_str("#   ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn fuse_rule_to_n3(premise: &[Triple], prefixes: &BTreeMap<String, String>) -> String {
    if premise.is_empty() {
        return "true => false .".to_string();
    }
    let body = premise
        .iter()
        .map(|t| format!("  {}", triple_to_n3(prefixes, t)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{{\n{body}\n}} => false .")
}

/// Use explicit syntax only where implicit syntax would change the scope.
fn explicit_graph_for_printing(graph: &Graph) -> (Vec<String>, Vec<Triple>) {
    fn nested_reference(term: &Term, name: &Name) -> bool {
        match term {
            Term::Formula(g) => g.free_variables().contains(&Term::blank(name.clone())),
            Term::List(items) => items.iter().any(|t| nested_reference(t, name)),
            _ => false,
        }
    }
    let escaping: BTreeSet<_> = graph
        .existentials
        .iter()
        .filter(|n| {
            graph.iter().any(|t| {
                [&t.s, &t.p, &t.o]
                    .iter()
                    .any(|term| nested_reference(term, n))
            })
        })
        .cloned()
        .collect();
    let mut replacements = BTreeMap::new();
    let mut declarations = Vec::new();
    for (keyword, names, universal) in [
        ("@forAll", &graph.universals, true),
        ("@forSome", &escaping, false),
    ] {
        for name in names {
            // Byte encoding gives an injective, legal IRI even for standardized-apart names.
            let encoded: String = name.bytes().map(|b| format!("{:02x}", b)).collect();
            let iri = format!("urn:eyeron:quantified:{encoded}");
            declarations.push(format!("{keyword} <{iri}> ."));
            replacements.insert(
                if universal {
                    Term::var(name.clone())
                } else {
                    Term::blank(name.clone())
                },
                Term::iri(iri),
            );
        }
    }
    fn replace(term: &Term, replacements: &BTreeMap<Term, Term>) -> Term {
        if let Some(value) = replacements.get(term) {
            return value.clone();
        }
        match term {
            Term::Formula(g) => {
                let mut local = replacements.clone();
                for n in &g.universals {
                    local.remove(&Term::var(n.clone()));
                }
                for n in &g.existentials {
                    local.remove(&Term::blank(n.clone()));
                }
                Term::Formula(g.map_triples(|t| triple(t, &local)))
            }
            Term::List(items) => {
                Term::List(items.iter().map(|t| replace(t, replacements)).collect())
            }
            _ => term.clone(),
        }
    }
    fn triple(t: &Triple, replacements: &BTreeMap<Term, Term>) -> Triple {
        Triple::new(
            replace(&t.s, replacements),
            replace(&t.p, replacements),
            replace(&t.o, replacements),
        )
    }
    (
        declarations,
        graph.iter().map(|t| triple(t, &replacements)).collect(),
    )
}

fn formula_to_n3(graph: &Graph, prefixes: &BTreeMap<String, String>, indent: usize) -> String {
    if graph.is_empty() {
        return "true".to_string();
    }
    let (declarations, triples) = explicit_graph_for_printing(graph);
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 4);
    let mut out = String::new();
    out.push_str("{\n");
    for declaration in declarations {
        out.push_str(&inner);
        out.push_str(&declaration);
        out.push('\n');
    }
    for t in &triples {
        if is_implication_triple(t) {
            let rendered = implication_to_n3(t, prefixes);
            for line in rendered.lines() {
                out.push_str(&inner);
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(&inner);
            out.push_str(&format!(
                "{} {} {} .\n",
                term_to_n3(&t.s, prefixes, Position::Subject),
                term_to_n3(&t.p, prefixes, Position::Predicate),
                term_to_n3(&t.o, prefixes, Position::Object),
            ));
        }
    }
    out.push_str(&pad);
    out.push('}');
    out
}

/// A numeric literal may be written without its datatype only when the
/// shorthand reads back as the same datatype: an `INTEGER` has no `.` or
/// exponent, a `DECIMAL` has a `.`, a `DOUBLE` has an exponent (Turtle
/// grammar). `"72.0"^^xsd:double`
/// written bare would come back as an `xsd:decimal`, so it keeps its
/// datatype instead.
fn numeric_shorthand_round_trips(datatype: &str, value: &str) -> bool {
    let has_exponent = value.contains('e') || value.contains('E');
    let has_dot = value.contains('.');
    match datatype {
        "http://www.w3.org/2001/XMLSchema#integer" => !has_dot && !has_exponent,
        "http://www.w3.org/2001/XMLSchema#decimal" => has_dot && !has_exponent,
        "http://www.w3.org/2001/XMLSchema#double" => has_exponent,
        _ => false,
    }
}

fn literal_to_n3(lit: &Literal, prefixes: &BTreeMap<String, String>) -> String {
    match lit.datatype.as_deref() {
        Some(dt) if numeric_shorthand_round_trips(dt, &lit.value) => lit.value.clone().to_string(),
        Some("http://www.w3.org/2001/XMLSchema#boolean")
            if lit.value == "true" || lit.value == "false" =>
        {
            lit.value.clone().to_string()
        }
        Some(dt) => format!(
            "\"{}\"^^{}",
            escape_string(&lit.value),
            compact_iri(dt, prefixes).unwrap_or_else(|| format!("<{}>", dt))
        ),
        None => match &lit.language {
            Some(lang) => format!("\"{}\"@{}", escape_string(&lit.value), lang),
            None => format!("\"{}\"", escape_string(&lit.value)),
        },
    }
}

fn compact_iri(iri: &str, prefixes: &BTreeMap<String, String>) -> Option<String> {
    let mut best: Option<(&str, &str)> = None;
    for (prefix, base) in prefixes {
        if iri.starts_with(base) {
            let local = &iri[base.len()..];
            if !local.is_empty() && valid_local(local) {
                match best {
                    Some((_, best_base)) if best_base.len() >= base.len() => {}
                    _ => best = Some((prefix.as_str(), base.as_str())),
                }
            }
        }
    }
    best.map(|(prefix, base)| {
        let local = &iri[base.len()..];
        if prefix.is_empty() {
            format!(":{local}")
        } else {
            format!("{prefix}:{local}")
        }
    })
}

pub(crate) fn used_prefixes(
    prefixes: &BTreeMap<String, String>,
    triples: &[Triple],
) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    for t in triples {
        collect_used_prefixes(&t.s, Position::Subject, prefixes, &mut used);
        if !is_implication_triple(t) {
            collect_used_prefixes(&t.p, Position::Predicate, prefixes, &mut used);
        }
        collect_used_prefixes(&t.o, Position::Object, prefixes, &mut used);
    }
    used
}

fn used_prefixes_for_trig(
    prefixes: &BTreeMap<String, String>,
    triples: &[Triple],
) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    for t in triples {
        if let Some((graph, graph_triples)) = named_graph_fact(t) {
            collect_used_prefixes(graph, Position::Subject, prefixes, &mut used);
            for inner in graph_triples {
                collect_used_prefixes(&inner.s, Position::Subject, prefixes, &mut used);
                collect_used_prefixes(&inner.p, Position::Predicate, prefixes, &mut used);
                collect_used_prefixes(&inner.o, Position::Object, prefixes, &mut used);
            }
        } else {
            collect_used_prefixes(&t.s, Position::Subject, prefixes, &mut used);
            collect_used_prefixes(&t.p, Position::Predicate, prefixes, &mut used);
            collect_used_prefixes(&t.o, Position::Object, prefixes, &mut used);
        }
    }
    used
}

fn collect_used_prefixes(
    term: &Term,
    pos: Position,
    prefixes: &BTreeMap<String, String>,
    used: &mut BTreeSet<String>,
) {
    match term {
        Term::Iri(iri) => {
            if matches!(pos, Position::Predicate) && iri == RDF_TYPE {
                return;
            }
            if let Some((p, _)) = prefix_for_iri(iri, prefixes) {
                used.insert(p.to_string());
            }
        }
        Term::Literal(lit) => {
            if let Some(dt) = &lit.datatype {
                if let Some((p, _)) = prefix_for_iri(dt, prefixes) {
                    used.insert(p.to_string());
                }
            }
        }
        Term::List(items) => {
            for item in items {
                collect_used_prefixes(item, Position::Object, prefixes, used);
            }
        }
        Term::Formula(triples) => {
            for t in triples {
                collect_used_prefixes(&t.s, Position::Subject, prefixes, used);
                if !is_implication_triple(t) {
                    collect_used_prefixes(&t.p, Position::Predicate, prefixes, used);
                }
                collect_used_prefixes(&t.o, Position::Object, prefixes, used);
            }
        }
        _ => {}
    }
}

fn prefix_for_iri<'a>(
    iri: &str,
    prefixes: &'a BTreeMap<String, String>,
) -> Option<(&'a str, &'a str)> {
    let mut best: Option<(&str, &str)> = None;
    for (prefix, base) in prefixes {
        if iri.starts_with(base) {
            let local = &iri[base.len()..];
            if !local.is_empty() && valid_local(local) {
                match best {
                    Some((_, best_base)) if best_base.len() >= base.len() => {}
                    _ => best = Some((prefix.as_str(), base.as_str())),
                }
            }
        }
    }
    best
}

fn valid_local(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && !s.starts_with('.')
        && !s.ends_with('.')
}

fn sanitize_blank(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// NOTE: `escape_string` is the N3/Turtle *string* escape law (`\"`, `\\`,
// `\n`, `\r`, `\t`, everything else raw) — it is NOT JSON escaping and must
// never be swapped for `purrdf_lex::json_escape`. Out of scope here.
fn escape_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Renders a [`Document`] as a human-readable multi-line debug dump.
pub fn document_debug(doc: &Document) -> String {
    let mut out = String::new();
    out.push_str("Document {\n");
    out.push_str("  prefixes:\n");
    for (k, v) in &doc.prefixes {
        out.push_str(&format!("    {:?}: {:?}\n", k, v));
    }
    out.push_str(&format!("  base_iri: {:?}\n", doc.base_iri));
    out.push_str("  facts:\n");
    for t in &doc.facts {
        out.push_str(&format!("    {:?}\n", t));
    }
    out.push_str("  rules:\n");
    for r in &doc.rules {
        out.push_str(&format!("    {:?}\n", r));
    }
    out.push_str("}\n");
    out
}

/// Renders a document's triples as an RDF 1.2 JSON quad array string.
pub fn rdf12_json(doc: &Document) -> String {
    let mut writer = RdfJsonWriter {
        quads: Vec::new(),
        blank_counter: 0,
    };
    writer.document(doc);
    format!("[{}]\n", writer.quads.join(","))
}

struct RdfJsonWriter {
    quads: Vec<String>,
    blank_counter: usize,
}

impl RdfJsonWriter {
    fn document(&mut self, doc: &Document) {
        for triple in &doc.facts {
            if let Some((graph, triples)) = named_graph_fact(triple) {
                let graph_json = self.graph_term_json(graph);
                for inner in triples {
                    self.triple(inner, &graph_json);
                }
            } else {
                let graph_json = default_graph_json();
                self.triple(triple, &graph_json);
            }
        }
    }

    fn triple(&mut self, triple: &Triple, graph_json: &str) {
        let s = self.term_json(&triple.s, JsonPosition::Subject, graph_json);
        let p = self.term_json(&triple.p, JsonPosition::Predicate, graph_json);
        let o = self.term_json(&triple.o, JsonPosition::Object, graph_json);
        self.quads.push(quad_json(&s, &p, &o, graph_json));
    }

    fn term_json(&mut self, term: &Term, position: JsonPosition, graph_json: &str) -> String {
        match term {
            Term::Iri(iri) => named_node_json(iri),
            Term::Blank(id) => blank_node_json(id),
            Term::Literal(lit) => literal_json(lit),
            Term::List(items) => self.list_json(items, graph_json),
            Term::Formula(triples)
                if triples.len() == 1
                    && !matches!(position, JsonPosition::Predicate | JsonPosition::Graph) =>
            {
                let triple = &triples[0];
                let s = self.term_json(&triple.s, JsonPosition::Subject, graph_json);
                let p = self.term_json(&triple.p, JsonPosition::Predicate, graph_json);
                let o = self.term_json(&triple.o, JsonPosition::Object, graph_json);
                quad_json(&s, &p, &o, &default_graph_json())
            }
            Term::Formula(_) => blank_node_json(&self.fresh_blank_id("unsupportedFormula")),
            Term::Var(name) => blank_node_json(&format!("var_{}", sanitize_blank(name))),
        }
    }

    fn graph_term_json(&mut self, term: &Term) -> String {
        match term {
            Term::Iri(_) | Term::Blank(_) => {
                self.term_json(term, JsonPosition::Graph, &default_graph_json())
            }
            _ => blank_node_json(&self.fresh_blank_id("graph")),
        }
    }

    fn list_json(&mut self, items: &[Term], graph_json: &str) -> String {
        if items.is_empty() {
            return named_node_json(RDF_NIL);
        }
        let nodes: Vec<String> = (0..items.len())
            .map(|_| self.fresh_blank_id("rdfList"))
            .collect();
        for (idx, item) in items.iter().enumerate() {
            let subject = blank_node_json(&nodes[idx]);
            let value = self.term_json(item, JsonPosition::Object, graph_json);
            let rest = if idx + 1 < nodes.len() {
                blank_node_json(&nodes[idx + 1])
            } else {
                named_node_json(RDF_NIL)
            };
            self.quads.push(quad_json(
                &subject,
                &named_node_json(RDF_FIRST),
                &value,
                graph_json,
            ));
            self.quads.push(quad_json(
                &subject,
                &named_node_json(RDF_REST),
                &rest,
                graph_json,
            ));
        }
        blank_node_json(&nodes[0])
    }

    fn fresh_blank_id(&mut self, prefix: &str) -> String {
        self.blank_counter += 1;
        format!("{}{}", prefix, self.blank_counter)
    }
}

#[derive(Clone, Copy)]
enum JsonPosition {
    Subject,
    Predicate,
    Object,
    Graph,
}

fn named_graph_fact(triple: &Triple) -> Option<(&Term, &[Triple])> {
    match (&triple.p, &triple.o) {
        (Term::Iri(p), Term::Formula(triples)) if p == LOG_NAME_OF => Some((&triple.s, triples)),
        _ => None,
    }
}

fn named_node_json(value: &str) -> String {
    let mut out = String::from("{\"termType\":\"NamedNode\",\"value\":\"");
    push_body(&mut out, value, JsonEscapes::Minimal);
    out.push_str("\"}");
    out
}

fn blank_node_json(value: &str) -> String {
    let mut out = String::from("{\"termType\":\"BlankNode\",\"value\":\"");
    push_body(&mut out, value, JsonEscapes::Minimal);
    out.push_str("\"}");
    out
}

fn literal_json(lit: &Literal) -> String {
    let (language, datatype) = match lit.language.as_deref() {
        Some(lang) if lang.contains("--") => (lang, purrdf_iri::vocab::rdf::DIR_LANG_STRING),
        Some(lang) => (lang, purrdf_iri::vocab::rdf::LANG_STRING),
        None => (
            "",
            lit.datatype
                .as_deref()
                .unwrap_or("http://www.w3.org/2001/XMLSchema#string"),
        ),
    };
    let mut out = String::from("{\"termType\":\"Literal\",\"value\":\"");
    push_body(&mut out, &lit.value, JsonEscapes::Minimal);
    out.push_str("\",\"language\":\"");
    push_body(&mut out, language, JsonEscapes::Minimal);
    out.push_str("\",\"datatype\":");
    out.push_str(&named_node_json(datatype));
    out.push('}');
    out
}

fn default_graph_json() -> String {
    "{\"termType\":\"DefaultGraph\",\"value\":\"\"}".to_string()
}

#[cfg(test)]
mod golden_json_escapes {
    use super::*;

    /// OPT-17 part 3 golden: the RDF 1.2 JSON writer escapes its string bodies
    /// with `purrdf_lex::json_escape::JsonEscapes::Minimal`, NOT the deleted
    /// hand-rolled `escape_json`. The deliberate public byte change is confined
    /// to C0 controls: `\b` -> `\u0008`, `\f` -> `\u000c`, and every other
    /// non-short-form C0 escape goes lowercase hex (was uppercase `\u{:04X}`).
    /// DEL (U+007F) and the C1 block stay raw in both spellings. This fixture
    /// pins the exact new spelling so the change is deliberate, not silent.
    #[test]
    fn rdf12_json_uses_lex_minimal_escapes() {
        let value = format!("q\"b\\n\nt\u{7}\u{8}\u{c}\u{1f}\u{7f}");
        let doc = Document {
            prefixes: BTreeMap::new(),
            base_iri: None,
            facts: vec![Triple {
                s: Term::Iri(Name::from("http://example.org/s")),
                p: Term::Iri(Name::from("http://example.org/p")),
                o: Term::Literal(Literal {
                    value: Name::from(value.as_str()),
                    datatype: None,
                    language: None,
                }),
            }],
            universals: BTreeSet::new(),
            existentials: BTreeSet::new(),
            fact_sources: BTreeMap::new(),
            rules: Vec::new(),
        };
        let json = rdf12_json(&doc);
        let expected = concat!(
            "[{\"termType\":\"Quad\",\"value\":\"\",",
            "\"subject\":{\"termType\":\"NamedNode\",\"value\":\"http://example.org/s\"},",
            "\"predicate\":{\"termType\":\"NamedNode\",\"value\":\"http://example.org/p\"},",
            "\"object\":{\"termType\":\"Literal\",\"value\":\"q\\\"b\\\\n\\nt\\u0007\\u0008\\u000c\\u001f\u{7f}\",",
            "\"language\":\"\",\"datatype\":{\"termType\":\"NamedNode\",",
            "\"value\":\"http://www.w3.org/2001/XMLSchema#string\"}},",
            "\"graph\":{\"termType\":\"DefaultGraph\",\"value\":\"\"}}]\n"
        );
        assert_eq!(json, expected);
    }
}

fn quad_json(subject: &str, predicate: &str, object: &str, graph: &str) -> String {
    format!(
        "{{\"termType\":\"Quad\",\"value\":\"\",\"subject\":{},\"predicate\":{},\"object\":{},\"graph\":{}}}",
        subject, predicate, object, graph
    )
}
