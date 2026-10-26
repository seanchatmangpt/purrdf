use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

pub const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
pub const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
pub const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
pub const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";
pub const LOG_OUTPUT_STRING: &str = "http://www.w3.org/2000/10/swap/log#outputString";
pub const LOG_QUERY: &str = "http://www.w3.org/2000/10/swap/log#query";
pub const LOG_EQUAL_TO: &str = "http://www.w3.org/2000/10/swap/log#equalTo";
pub const LOG_NOT_EQUAL_TO: &str = "http://www.w3.org/2000/10/swap/log#notEqualTo";
pub const LOG_IMPLIES: &str = "http://www.w3.org/2000/10/swap/log#implies";
/// Standard SWAP predicate used to preserve N3 `<=` rule polarity.
/// It is printed as `<=` and promoted to a backward rule.
pub const LOG_IMPLIED_BY: &str = "http://www.w3.org/2000/10/swap/log#impliedBy";
/// Internal marker used for rule conclusions like `=> ?F`, where the
/// RHS resolves to a quoted formula whose contents should be unquoted.
pub const EYERON_UNQUOTE: &str = "urn:eyeron:unquote";
pub const LOG_COLLECT_ALL_IN: &str = "http://www.w3.org/2000/10/swap/log#collectAllIn";
pub const LOG_FOR_ALL_IN: &str = "http://www.w3.org/2000/10/swap/log#forAllIn";
pub const LOG_CONCLUSION: &str = "http://www.w3.org/2000/10/swap/log#conclusion";
pub const LOG_CONJUNCTION: &str = "http://www.w3.org/2000/10/swap/log#conjunction";
pub const LOG_INCLUDES: &str = "http://www.w3.org/2000/10/swap/log#includes";
pub const LOG_NOT_INCLUDES: &str = "http://www.w3.org/2000/10/swap/log#notIncludes";
pub const LOG_NAME_OF: &str = "http://www.w3.org/2000/10/swap/log#nameOf";
pub const LOG_URI: &str = "http://www.w3.org/2000/10/swap/log#uri";
pub const LOG_RAW_TYPE: &str = "http://www.w3.org/2000/10/swap/log#rawType";
pub const LOG_DTLIT: &str = "http://www.w3.org/2000/10/swap/log#dtlit";
pub const LOG_LANGLIT: &str = "http://www.w3.org/2000/10/swap/log#langlit";
pub const LOG_CONTENT: &str = "http://www.w3.org/2000/10/swap/log#content";
pub const LOG_SEMANTICS: &str = "http://www.w3.org/2000/10/swap/log#semantics";
pub const LOG_SEMANTICS_OR_ERROR: &str = "http://www.w3.org/2000/10/swap/log#semanticsOrError";
pub const LOG_PARSED_AS_N3: &str = "http://www.w3.org/2000/10/swap/log#parsedAsN3";
pub const LOG_SKOLEM: &str = "http://www.w3.org/2000/10/swap/log#skolem";
pub const LOG_UUID: &str = "http://www.w3.org/2000/10/swap/log#uuid";
pub const DT_DATATYPE: &str = "https://eyereasoner.github.io/eyeron/datatype#datatype";
pub const DT_LEXICAL_FORM: &str = "https://eyereasoner.github.io/eyeron/datatype#lexicalForm";
pub const EYELING_DT_DATATYPE: &str = "https://eyereasoner.github.io/eyeling/datatype#datatype";
pub const EYELING_DT_LEXICAL_FORM: &str = "https://eyereasoner.github.io/eyeling/datatype#lexicalForm";
pub const OWL_SAME_AS: &str = "http://www.w3.org/2002/07/owl#sameAs";
pub const MATH_SUM: &str = "http://www.w3.org/2000/10/swap/math#sum";
pub const MATH_GREATER_THAN: &str = "http://www.w3.org/2000/10/swap/math#greaterThan";
pub const MATH_LESS_THAN: &str = "http://www.w3.org/2000/10/swap/math#lessThan";
pub const MATH_NOT_GREATER_THAN: &str = "http://www.w3.org/2000/10/swap/math#notGreaterThan";
pub const MATH_NOT_LESS_THAN: &str = "http://www.w3.org/2000/10/swap/math#notLessThan";
pub const MATH_EQUAL_TO: &str = "http://www.w3.org/2000/10/swap/math#equalTo";
pub const MATH_NOT_EQUAL_TO: &str = "http://www.w3.org/2000/10/swap/math#notEqualTo";
pub const MATH_DIFFERENCE: &str = "http://www.w3.org/2000/10/swap/math#difference";
pub const MATH_PRODUCT: &str = "http://www.w3.org/2000/10/swap/math#product";
pub const MATH_QUOTIENT: &str = "http://www.w3.org/2000/10/swap/math#quotient";
pub const MATH_INTEGER_QUOTIENT: &str = "http://www.w3.org/2000/10/swap/math#integerQuotient";
pub const MATH_REMAINDER: &str = "http://www.w3.org/2000/10/swap/math#remainder";
pub const MATH_EXPONENTIATION: &str = "http://www.w3.org/2000/10/swap/math#exponentiation";
pub const MATH_NEGATION: &str = "http://www.w3.org/2000/10/swap/math#negation";
pub const MATH_ABSOLUTE_VALUE: &str = "http://www.w3.org/2000/10/swap/math#absoluteValue";
pub const MATH_ROUNDED: &str = "http://www.w3.org/2000/10/swap/math#rounded";
pub const MATH_SIN: &str = "http://www.w3.org/2000/10/swap/math#sin";
pub const MATH_COS: &str = "http://www.w3.org/2000/10/swap/math#cos";
pub const MATH_TAN: &str = "http://www.w3.org/2000/10/swap/math#tan";
pub const MATH_ASIN: &str = "http://www.w3.org/2000/10/swap/math#asin";
pub const MATH_ACOS: &str = "http://www.w3.org/2000/10/swap/math#acos";
pub const MATH_ATAN: &str = "http://www.w3.org/2000/10/swap/math#atan";
pub const MATH_SINH: &str = "http://www.w3.org/2000/10/swap/math#sinh";
pub const MATH_COSH: &str = "http://www.w3.org/2000/10/swap/math#cosh";
pub const MATH_TANH: &str = "http://www.w3.org/2000/10/swap/math#tanh";
pub const MATH_DEGREES: &str = "http://www.w3.org/2000/10/swap/math#degrees";
pub const CRYPTO_SHA: &str = "http://www.w3.org/2000/10/swap/crypto#sha";
pub const STRING_LESS_THAN: &str = "http://www.w3.org/2000/10/swap/string#lessThan";
pub const STRING_GREATER_THAN: &str = "http://www.w3.org/2000/10/swap/string#greaterThan";
pub const STRING_NOT_LESS_THAN: &str = "http://www.w3.org/2000/10/swap/string#notLessThan";
pub const STRING_NOT_GREATER_THAN: &str = "http://www.w3.org/2000/10/swap/string#notGreaterThan";
pub const STRING_CONCATENATION: &str = "http://www.w3.org/2000/10/swap/string#concatenation";
pub const STRING_CONTAINS: &str = "http://www.w3.org/2000/10/swap/string#contains";
pub const STRING_CONTAINS_IGNORING_CASE: &str = "http://www.w3.org/2000/10/swap/string#containsIgnoringCase";
pub const STRING_ENDS_WITH: &str = "http://www.w3.org/2000/10/swap/string#endsWith";
pub const STRING_STARTS_WITH: &str = "http://www.w3.org/2000/10/swap/string#startsWith";
pub const STRING_EQUAL_IGNORING_CASE: &str = "http://www.w3.org/2000/10/swap/string#equalIgnoringCase";
pub const STRING_NOT_EQUAL_IGNORING_CASE: &str = "http://www.w3.org/2000/10/swap/string#notEqualIgnoringCase";
pub const STRING_FORMAT: &str = "http://www.w3.org/2000/10/swap/string#format";
pub const STRING_MATCHES: &str = "http://www.w3.org/2000/10/swap/string#matches";
pub const STRING_NOT_MATCHES: &str = "http://www.w3.org/2000/10/swap/string#notMatches";
pub const STRING_REPLACE: &str = "http://www.w3.org/2000/10/swap/string#replace";
pub const STRING_SCRAPE: &str = "http://www.w3.org/2000/10/swap/string#scrape";
/// Non-standard extension, not part of the N3 builtin vocabulary proper:
/// solves a 9x9 Sudoku puzzle (see `crate::sudoku`).
pub const SUDOKU_SOLVE: &str = "https://eyereasoner.github.io/eyeling/vocab/sudoku#solve";
pub const LIST_APPEND: &str = "http://www.w3.org/2000/10/swap/list#append";
pub const LIST_ITERATE: &str = "http://www.w3.org/2000/10/swap/list#iterate";
pub const LIST_MAP: &str = "http://www.w3.org/2000/10/swap/list#map";
pub const LIST_FIRST_REST: &str = "http://www.w3.org/2000/10/swap/list#firstRest";
pub const LIST_REVERSE: &str = "http://www.w3.org/2000/10/swap/list#reverse";
pub const LIST_SORT: &str = "http://www.w3.org/2000/10/swap/list#sort";
pub const LIST_NOT_MEMBER: &str = "http://www.w3.org/2000/10/swap/list#notMember";
pub const LIST_FIRST: &str = "http://www.w3.org/2000/10/swap/list#first";
pub const LIST_REST: &str = "http://www.w3.org/2000/10/swap/list#rest";
pub const LIST_LAST: &str = "http://www.w3.org/2000/10/swap/list#last";
pub const LIST_LENGTH: &str = "http://www.w3.org/2000/10/swap/list#length";
pub const LIST_MEMBER: &str = "http://www.w3.org/2000/10/swap/list#member";
pub const LIST_IN: &str = "http://www.w3.org/2000/10/swap/list#in";
pub const LIST_MEMBER_AT: &str = "http://www.w3.org/2000/10/swap/list#memberAt";
pub const LIST_REMOVE: &str = "http://www.w3.org/2000/10/swap/list#remove";

pub const TIME_YEAR: &str = "http://www.w3.org/2000/10/swap/time#year";
pub const TIME_MONTH: &str = "http://www.w3.org/2000/10/swap/time#month";
pub const TIME_DAY: &str = "http://www.w3.org/2000/10/swap/time#day";
pub const TIME_HOUR: &str = "http://www.w3.org/2000/10/swap/time#hour";
pub const TIME_MINUTE: &str = "http://www.w3.org/2000/10/swap/time#minute";
pub const TIME_SECOND: &str = "http://www.w3.org/2000/10/swap/time#second";
pub const TIME_TIME_ZONE: &str = "http://www.w3.org/2000/10/swap/time#timeZone";
pub const TIME_LOCAL_TIME: &str = "http://www.w3.org/2000/10/swap/time#localTime";

/// The text of an IRI, a variable, a blank node label or a literal.
///
/// A term's text never changes once it is read, so cloning a term should not
/// copy it: this is a shared, immutable string, and cloning one is a refcount
/// bump rather than an allocation. Profiling the reasoner found about 60% of
/// its instructions inside malloc and free, a third of the total in
/// `Term::clone` alone.
///
/// It derefs to `str` and compares with `str`, so it reads like the `String`
/// it replaced.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(Arc<str>);

impl Name {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl Default for Name {
    fn default() -> Self { Self(Arc::from("")) }
}

impl Deref for Name {
    type Target = str;
    fn deref(&self) -> &str { &self.0 }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str { &self.0 }
}

impl std::borrow::Borrow<str> for Name {
    fn borrow(&self) -> &str { &self.0 }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool { &*self.0 == other }
}

impl PartialEq<Name> for str {
    fn eq(&self, other: &Name) -> bool { self == &*other.0 }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool { &*self.0 == *other }
}

impl PartialEq<Name> for String {
    fn eq(&self, other: &Name) -> bool { self.as_str() == &*other.0 }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool { &*self.0 == other.as_str() }
}

impl From<&str> for Name {
    fn from(value: &str) -> Self { Self(Arc::from(value)) }
}

impl From<String> for Name {
    fn from(value: String) -> Self { Self(Arc::from(value)) }
}

impl From<&String> for Name {
    fn from(value: &String) -> Self { Self(Arc::from(value.as_str())) }
}

impl From<Name> for String {
    fn from(value: Name) -> Self { value.0.to_string() }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Debug::fmt(&*self.0, f) }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Literal {
    pub value: Name,
    pub datatype: Option<Name>,
    pub language: Option<Name>,
}

impl Literal {
    pub fn plain(value: impl Into<Name>) -> Self {
        Self { value: value.into(), datatype: None, language: None }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Term {
    Iri(Name),
    Var(Name),
    Blank(Name),
    Literal(Literal),
    List(Vec<Term>),
    Formula(Graph),
}

impl fmt::Debug for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iri(v) => f.debug_tuple("Iri").field(v).finish(),
            Self::Var(v) => f.debug_tuple("Var").field(v).finish(),
            Self::Blank(v) => f.debug_tuple("Blank").field(v).finish(),
            Self::Literal(v) => f.debug_tuple("Literal").field(v).finish(),
            Self::List(v) => f.debug_tuple("List").field(v).finish(),
            // Retain the historical representation used for deterministic
            // witness hashes when there is no local universal declaration.
            Self::Formula(g) if g.universals.is_empty() => f.debug_tuple("Formula").field(&g.triples).finish(),
            Self::Formula(g) => f.debug_tuple("Formula").field(g).finish(),
        }
    }
}

impl Term {
    pub fn iri(value: impl Into<Name>) -> Self { Self::Iri(value.into()) }
    pub fn var(value: impl Into<Name>) -> Self { Self::Var(value.into()) }
    pub fn blank(value: impl Into<Name>) -> Self { Self::Blank(value.into()) }
    pub fn literal(value: impl Into<Name>) -> Self { Self::Literal(Literal::plain(value)) }
    pub fn list(items: Vec<Term>) -> Self { Self::List(items) }
    pub fn formula(triples: impl Into<Graph>) -> Self { Self::Formula(triples.into()) }

    pub fn is_variable(&self) -> bool { matches!(self, Term::Var(_)) }
    pub fn is_ground(&self) -> bool {
        match self {
            Term::Var(_) => false,
            Term::List(items) => items.iter().all(Term::is_ground),
            Term::Formula(graph) => graph.free_variables().is_empty(),
            _ => true,
        }
    }

    /// The canonical identity serialization of this term.
    ///
    /// Identity-sensitive consumers hash THIS string, never the `Debug`
    /// rendering: `Debug` is diagnostic output and drops a formula's
    /// existential binder set whenever the formula declares no universals,
    /// so two distinct formulas (a blank bound in-formula versus the same
    /// blank bound by an outer scope) used to collide on one skolem IRI,
    /// one backward-variable salt, and one proof dedup key.
    ///
    /// Every field of every variant is covered:
    ///
    /// - `Iri(v)` -> `<v>`, `\` and `>` escaped
    /// - `Var(n)` -> `?` + escaped name
    /// - `Blank(n)` -> `_:name`, name escaped
    /// - `Literal` -> `"value"`, then `^^<datatype>` or `@lang`
    /// - `List(items)` -> `( item ... )`
    /// - `Formula(g)` -> `{ U[u ...] E[e ...] T[triple ...] }`: both binder
    ///   sets (sorted, as `BTreeSet` iterates) and every triple, recursively
    ///   serialized. The binder sets are identity-relevant: a formula owns
    ///   its quantifier scope.
    ///
    /// Atomic text (IRIs, names, literal values, languages, datatypes)
    /// escapes `\` and every structural character of this grammar, so text
    /// cannot masquerade as structure and distinct terms never share a
    /// serialization.
    pub fn canonical_form(&self) -> String {
        match self {
            Self::Iri(value) => format!("<{}>", canonical_escape(value)),
            Self::Var(name) => format!("?{}", canonical_escape(name)),
            Self::Blank(name) => format!("_:{}", canonical_escape(name)),
            Self::Literal(lit) => {
                let mut out = String::from("\"");
                out.push_str(&canonical_escape(&lit.value));
                out.push('"');
                if let Some(language) = &lit.language {
                    out.push('@');
                    out.push_str(&canonical_escape(language));
                } else if let Some(datatype) = &lit.datatype {
                    out.push_str("^^<");
                    out.push_str(&canonical_escape(datatype));
                    out.push('>');
                }
                out
            }
            Self::List(items) => {
                if items.is_empty() { return "( )".to_string(); }
                let mut out = String::from("( ");
                for item in items {
                    out.push_str(&item.canonical_form());
                    out.push(' ');
                }
                out.push(')');
                out
            }
            Self::Formula(graph) => {
                let mut out = String::from("{ U[");
                for name in &graph.universals {
                    out.push_str(&canonical_escape(name));
                    out.push(' ');
                }
                out.push_str("] E[");
                for name in &graph.existentials {
                    out.push_str(&canonical_escape(name));
                    out.push(' ');
                }
                out.push_str("] T[");
                for triple in &graph.triples {
                    out.push_str(&triple.canonical_form());
                    out.push(' ');
                }
                out.push_str("] }");
                out
            }
        }
    }
}

/// `\` and the structural characters of `Term::canonical_form`'s grammar,
/// escaped in every atomic text position.
fn canonical_escape(text: &str) -> String {
    const STRUCTURAL: &[char] = &[
        '\\', '<', '>', '?', '_', ':', '"', '@', '^', '(', ')', '{', '}', '[', ']', ' ', '\t', '\n', '\r',
    ];
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if STRUCTURAL.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A graph is ∀universals ∃existentials { triples }.
/// Bound names are parser-generated identities, not the source IRIs.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Graph {
    pub universals: BTreeSet<Name>,
    pub existentials: BTreeSet<Name>,
    pub triples: Vec<Triple>,
}

impl Graph {
    pub fn free_variables(&self) -> BTreeSet<Term> {
        let mut vars = BTreeSet::new();
        for triple in &self.triples {
            for term in [&triple.s, &triple.p, &triple.o] {
                term.collect_free_variables(&mut vars);
            }
        }
        vars.retain(|v| match v {
            Term::Var(n) => !self.universals.contains(n),
            Term::Blank(n) => !self.existentials.contains(n),
            _ => true,
        });
        vars
    }

    /// Apply a substitution to the triples, masking bindings of nested graphs.
    /// Callers replacing this graph's own binders manage its U/E sets separately.
    pub fn apply_to_triples(&self, mapping: &BTreeMap<Term, Term>) -> Self {
        fn term(t: &Term, mapping: &BTreeMap<Term, Term>) -> Term {
            if let Some(value) = mapping.get(t) {
                return value.clone();
            }
            match t {
                Term::List(items) => Term::List(items.iter().map(|t| term(t, mapping)).collect()),
                Term::Formula(g) => {
                    let mut local = mapping.clone();
                    for n in &g.universals {
                        local.remove(&Term::var(n.clone()));
                    }
                    for n in &g.existentials {
                        local.remove(&Term::blank(n.clone()));
                    }
                    Term::Formula(g.apply_to_triples(&local))
                }
                _ => t.clone(),
            }
        }
        self.map_triples(|t| {
            Triple::new(
                term(&t.s, mapping),
                term(&t.p, mapping),
                term(&t.o, mapping),
            )
        })
    }

    /// Capture-avoiding substitution.
    pub fn substitute(&self, mapping: &BTreeMap<Term, Term>) -> Self {
        let mut local = mapping.clone();
        for n in &self.universals {
            local.remove(&Term::var(n.clone()));
        }
        for n in &self.existentials {
            local.remove(&Term::blank(n.clone()));
        }
        self.apply_to_triples(&local)
    }

    /// Transform triples without discarding their quantifier scope.
    pub fn map_triples(&self, f: impl FnMut(&Triple) -> Triple) -> Self {
        Self {
            triples: self.triples.iter().map(f).collect(),
            ..self.clone()
        }
    }
}

impl Term {
    fn collect_free_variables(&self, vars: &mut BTreeSet<Term>) {
        match self {
            Self::Var(_) | Self::Blank(_) => {
                vars.insert(self.clone());
            }
            Self::List(items) => {
                for item in items {
                    item.collect_free_variables(vars);
                }
            }
            Self::Formula(graph) => vars.extend(graph.free_variables()),
            _ => {}
        }
    }
}

impl From<Vec<Triple>> for Graph {
    fn from(triples: Vec<Triple>) -> Self {
        fn blanks(term: &Term, names: &mut BTreeSet<Name>) {
            match term {
                Term::Blank(n) => {
                    names.insert(n.clone());
                }
                Term::List(items) => {
                    for item in items {
                        blanks(item, names);
                    }
                }
                Term::Formula(g) => {
                    for v in g.free_variables() {
                        if let Term::Blank(n) = v {
                            names.insert(n);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut graph = Self {
            triples,
            ..Self::default()
        };
        for t in &graph.triples {
            for term in [&t.s, &t.p, &t.o] {
                blanks(term, &mut graph.existentials);
            }
        }
        graph
    }
}
impl FromIterator<Triple> for Graph {
    fn from_iter<T: IntoIterator<Item = Triple>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}
impl Deref for Graph {
    type Target = Vec<Triple>;
    fn deref(&self) -> &Self::Target {
        &self.triples
    }
}
impl std::ops::DerefMut for Graph {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.triples
    }
}
impl IntoIterator for Graph {
    type Item = Triple;
    type IntoIter = std::vec::IntoIter<Triple>;
    fn into_iter(self) -> Self::IntoIter {
        self.triples.into_iter()
    }
}
impl<'a> IntoIterator for &'a Graph {
    type Item = &'a Triple;
    type IntoIter = std::slice::Iter<'a, Triple>;
    fn into_iter(self) -> Self::IntoIter {
        self.triples.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Triple {
    pub s: Term,
    pub p: Term,
    pub o: Term,
}

impl Triple {
    pub fn new(s: Term, p: Term, o: Term) -> Self { Self { s, p, o } }

    pub fn is_ground(&self) -> bool {
        self.s.is_ground() && self.p.is_ground() && self.o.is_ground()
    }

    /// The canonical identity serialization of this triple: the three
    /// terms' `canonical_form` texts, space-joined in subject/predicate/
    /// object order. Identity-sensitive consumers (proof-step dedup,
    /// backward-variable salting) key on this, never on `Debug`.
    pub fn canonical_form(&self) -> String {
        format!(
            "{} {} {}",
            self.s.canonical_form(),
            self.p.canonical_form(),
            self.o.canonical_form()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceRef {
    pub label: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub premise: Graph,
    pub conclusion: Graph,
    pub is_forward: bool,
    pub is_query: bool,
    /// `{ .. } => false`: an inference fuse. Its premise becoming provable is
    /// the thing the rule forbids, so there is nothing to conclude -- the run
    /// stops and reports which rule fired.
    pub is_fuse: bool,
    pub source: Option<SourceRef>,
    pub proof_var_source_names: BTreeMap<String, String>,
}

impl Rule {
    pub fn new(premise: impl Into<Graph>, conclusion: impl Into<Graph>, is_forward: bool) -> Self {
        Self { premise: premise.into(), conclusion: conclusion.into(), is_forward, is_query: false, is_fuse: false, source: None, proof_var_source_names: BTreeMap::new() }
    }

    pub fn fuse(premise: impl Into<Graph>) -> Self {
        Self { is_fuse: true, ..Self::new(premise, Vec::new(), true) }
    }

    pub fn with_source(mut self, source: Option<SourceRef>) -> Self {
        self.source = source;
        self
    }

    pub fn with_fuse(mut self, is_fuse: bool) -> Self {
        self.is_fuse = is_fuse;
        self
    }

    pub fn with_query(mut self, is_query: bool) -> Self {
        self.is_query = is_query;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub prefixes: BTreeMap<String, String>,
    pub base_iri: Option<String>,
    pub facts: Vec<Triple>,
    pub universals: BTreeSet<Name>,
    pub existentials: BTreeSet<Name>,
    pub fact_sources: BTreeMap<Triple, SourceRef>,
    pub rules: Vec<Rule>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            prefixes: default_prefixes(),
            base_iri: None,
            facts: Vec::new(),
            universals: BTreeSet::new(),
            existentials: BTreeSet::new(),
            fact_sources: BTreeMap::new(),
            rules: Vec::new(),
        }
    }

    pub fn merge(&mut self, mut other: Document) {
        // Independently parsed documents have disjoint binders, so the other
        // document's root binders that collide with ours get fresh names.
        // A root existential can reach into rules (through a root
        // `@forSome`), so every colliding one is renamed.  A root universal
        // only ties statements together through the root facts -- each rule
        // is standardized apart when it fires -- so `?x` is renamed only when
        // both documents use it in facts, and rule variables keep the names
        // proofs show.
        let shared: BTreeSet<Name> = other.universals.intersection(&self.universals).cloned().collect();
        let universals = if shared.is_empty() {
            shared
        } else {
            fact_variables(&other.facts, &fact_variables(&self.facts, &shared))
        };
        let taken = |n: &Name| {
            self.universals.contains(n)
                || self.existentials.contains(n)
                || other.universals.contains(n)
                || other.existentials.contains(n)
        };
        let mut renaming = Renaming::default();
        let mut fresh_names = BTreeSet::new();
        for (names, universal) in [(&universals, true), (&other.existentials, false)] {
            for name in names {
                if !universal && !self.existentials.contains(name) && !self.universals.contains(name) {
                    continue;
                }
                let mut suffix = 1;
                let fresh = loop {
                    let candidate: Name = format!("{}__merge{}", name, suffix).into();
                    if !taken(&candidate) && fresh_names.insert(candidate.clone()) {
                        break candidate;
                    }
                    suffix += 1;
                };
                if universal { &mut renaming.vars } else { &mut renaming.blanks }.insert(name.clone(), fresh);
            }
        }
        if !renaming.is_empty() {
            other.facts = other.facts.iter().map(|t| renaming.triple(t)).collect();
            other.fact_sources = std::mem::take(&mut other.fact_sources)
                .into_iter()
                .map(|(t, source)| (renaming.triple(&t), source))
                .collect();
            for rule in &mut other.rules {
                rule.premise = renaming.graph(&rule.premise);
                rule.conclusion = renaming.graph(&rule.conclusion);
            }
            let rename = |names: &BTreeSet<Name>, map: &BTreeMap<Name, Name>| -> BTreeSet<Name> {
                names.iter().map(|n| map.get(n).unwrap_or(n).clone()).collect()
            };
            other.universals = rename(&other.universals, &renaming.vars);
            other.existentials = rename(&other.existentials, &renaming.blanks);
        }
        for (k, v) in other.prefixes {
            self.prefixes.insert(k, v);
        }
        if self.base_iri.is_none() {
            self.base_iri = other.base_iri;
        }
        self.facts.extend(other.facts);
        self.universals.extend(other.universals);
        self.existentials.extend(other.existentials);
        self.fact_sources.extend(other.fact_sources);
        self.rules.extend(other.rules);
    }
}

impl Default for Document {
    fn default() -> Self { Self::new() }
}

/// The names in `wanted` that occur as variables anywhere in `facts`.
fn fact_variables(facts: &[Triple], wanted: &BTreeSet<Name>) -> BTreeSet<Name> {
    fn walk(t: &Term, wanted: &BTreeSet<Name>, found: &mut BTreeSet<Name>) {
        match t {
            Term::Var(n) if wanted.contains(n) => {
                found.insert(n.clone());
            }
            Term::List(items) => items.iter().for_each(|t| walk(t, wanted, found)),
            Term::Formula(g) => {
                for t in &g.triples {
                    [&t.s, &t.p, &t.o].into_iter().for_each(|t| walk(t, wanted, found));
                }
            }
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    for t in facts {
        if found.len() == wanted.len() {
            break;
        }
        [&t.s, &t.p, &t.o].into_iter().for_each(|t| walk(t, wanted, &mut found));
    }
    found
}

/// A renaming of free variables and blank nodes, done in one walk over a
/// term.  A nested graph that binds one of the renamed names masks it; the
/// walk keeps those graphs on a stack instead of copying the renaming.
#[derive(Default)]
struct Renaming {
    vars: BTreeMap<Name, Name>,
    blanks: BTreeMap<Name, Name>,
}

impl Renaming {
    fn is_empty(&self) -> bool { self.vars.is_empty() && self.blanks.is_empty() }

    fn triple(&self, t: &Triple) -> Triple { self.triple_in(t, &mut Vec::new()) }

    /// Rename inside `g`, leaving the names `g` itself binds alone.
    fn graph(&self, g: &Graph) -> Graph { self.graph_in(g, &mut Vec::new()) }

    fn triple_in<'g>(&self, t: &'g Triple, masks: &mut Vec<&'g Graph>) -> Triple {
        Triple::new(self.term_in(&t.s, masks), self.term_in(&t.p, masks), self.term_in(&t.o, masks))
    }

    fn term_in<'g>(&self, t: &'g Term, masks: &mut Vec<&'g Graph>) -> Term {
        match t {
            Term::Var(n) => match self.vars.get(n) {
                Some(v) if !masks.iter().any(|g| g.universals.contains(n)) => Term::Var(v.clone()),
                _ => t.clone(),
            },
            Term::Blank(n) => match self.blanks.get(n) {
                Some(v) if !masks.iter().any(|g| g.existentials.contains(n)) => Term::Blank(v.clone()),
                _ => t.clone(),
            },
            Term::List(items) => Term::List(items.iter().map(|t| self.term_in(t, masks)).collect()),
            Term::Formula(g) => Term::Formula(self.graph_in(g, masks)),
            _ => t.clone(),
        }
    }

    fn graph_in<'g>(&self, g: &'g Graph, masks: &mut Vec<&'g Graph>) -> Graph {
        let masking = g.universals.iter().any(|n| self.vars.contains_key(n))
            || g.existentials.iter().any(|n| self.blanks.contains_key(n));
        if masking {
            masks.push(g);
        }
        let out = Graph {
            universals: g.universals.clone(),
            existentials: g.existentials.clone(),
            triples: g.triples.iter().map(|t| self.triple_in(t, masks)).collect(),
        };
        if masking {
            masks.pop();
        }
        out
    }
}

pub fn default_prefixes() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("rdf".to_string(), "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string());
    m.insert("rdfs".to_string(), "http://www.w3.org/2000/01/rdf-schema#".to_string());
    m.insert("xsd".to_string(), "http://www.w3.org/2001/XMLSchema#".to_string());
    m.insert("log".to_string(), "http://www.w3.org/2000/10/swap/log#".to_string());
    m.insert("owl".to_string(), "http://www.w3.org/2002/07/owl#".to_string());
    m.insert("math".to_string(), "http://www.w3.org/2000/10/swap/math#".to_string());
    m.insert("string".to_string(), "http://www.w3.org/2000/10/swap/string#".to_string());
    m.insert("list".to_string(), "http://www.w3.org/2000/10/swap/list#".to_string());
    m.insert("time".to_string(), "http://www.w3.org/2000/10/swap/time#".to_string());
    m.insert("crypto".to_string(), "http://www.w3.org/2000/10/swap/crypto#".to_string());
    m.insert("eymsg".to_string(), "https://eyereasoner.github.io/eyeling/vocab/message#".to_string());
    m.insert("dt".to_string(), "https://eyereasoner.github.io/eyeron/datatype#".to_string());
    m.insert("genid".to_string(), "https://eyereasoner.github.io/.well-known/genid/".to_string());
    m
}

impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?} {:?}", self.s, self.p, self.o)
    }
}

#[cfg(test)]
mod constant_tests {
    use super::LOG_IMPLIED_BY;

    #[test]
    fn implied_by_uses_the_swap_log_iri() {
        assert_eq!(LOG_IMPLIED_BY, "http://www.w3.org/2000/10/swap/log#impliedBy");
    }
}

#[cfg(test)]
mod canonical_form_tests {
    use super::*;
    use crate::parser::parse_n3;

    fn name(s: &str) -> Name { Name::from(s) }

    /// F1: `Debug` drops a formula's existential binder set whenever no
    /// universals are declared (the historical rendering chosen for
    /// deterministic witness hashes), so two distinct formulas collide. The
    /// canonical identity serialization must not.
    #[test]
    fn binder_sets_survive_canonical_form() {
        let triples = || {
            vec![Triple::new(
                Term::Iri(name("http://example.org/x")),
                Term::Iri(name("http://example.org/p")),
                Term::Blank(name("q")),
            )]
        };
        let unbound = Graph { universals: BTreeSet::new(), existentials: BTreeSet::new(), triples: triples() };
        let existential = Graph { universals: BTreeSet::new(), existentials: [name("q")].into(), triples: triples() };
        let universal = Graph { universals: [name("u")].into(), existentials: BTreeSet::new(), triples: triples() };

        assert_ne!(
            Term::Formula(unbound.clone()).canonical_form(),
            Term::Formula(existential.clone()).canonical_form(),
            "the existential binder set is part of a formula's identity"
        );
        assert_ne!(
            Term::Formula(unbound.clone()).canonical_form(),
            Term::Formula(universal.clone()).canonical_form(),
            "the universal binder set is part of a formula's identity"
        );
        assert_eq!(
            Term::Formula(unbound.clone()).canonical_form(),
            Term::Formula(unbound.clone()).canonical_form(),
            "identical formulas serialize identically"
        );
        // And the serialized form stays stable through parse -> term: the
        // written formula `{ :x :p _:q }` declares `q` in-formula, so it
        // matches the hand-built existential twin exactly.
        let source = "@prefix : <http://example.org/> .\n{ :x :p _:q } log:skolem ?s .\n";
        let doc = parse_n3(source, None).expect("parses");
        let fact = &doc.facts[0];
        assert_eq!(
            fact.s.canonical_form(),
            Term::Formula(existential).canonical_form(),
            "a parsed formula and its hand-built twin serialize identically"
        );
    }

    /// The serialization is injective across variants: a literal whose text
    /// embeds structural characters must not read back as another term.
    #[test]
    fn structural_text_cannot_masquerade_as_structure() {
        let tricky = "x) { E[q] } U[ ?_:<>\"^^@ ( p ) ]";
        let literal = Term::Literal(Literal::plain(tricky));
        let list = Term::List(vec![Term::Literal(Literal::plain("x"))]);
        assert_ne!(
            literal.canonical_form(),
            Term::List(vec![literal.clone()]).canonical_form(),
        );
        assert_ne!(literal.canonical_form(), list.canonical_form());
        assert_eq!(literal.canonical_form(), Term::Literal(Literal::plain(tricky)).canonical_form());
        // A variable and a blank whose names embed each other's marker stay
        // distinct.
        assert_ne!(
            Term::Var(name("_:q")).canonical_form(),
            Term::Blank(name("q")).canonical_form()
        );
    }
}
