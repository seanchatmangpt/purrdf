//! Bridge between the N3 term model ([`crate::ast::Term`]) and the upstream
//! purrdf-rdf term model ([`purrdf_rdf::RdfTerm`]).
//!
//! Conversions are total: every case is either lossless or refuses with a
//! typed error — a case is never silently flattened. The typed edges are:
//!
//! - [`ConversionError::DirectionalLiteral`] — an upstream directional
//!   language-tagged literal (`rdf:dirLangString`) has no N3 representation
//!   (the N3 [`crate::ast::Literal`] carries no base direction).
//! - [`ConversionError::TripleTerm`] — an upstream RDF 1.2 triple term has no
//!   dedicated N3 `Term` variant; it is NOT flattened into a formula.
//! - [`ConversionError::Variable`], [`ConversionError::List`],
//!   [`ConversionError::Formula`] — N3-only term kinds with no RDF 1.2 term
//!   equivalent (an N3 list is NOT flattened into an RDF collection, which
//!   would mint nodes the source never declared).
//!
//! Language tags are folded to the RDF 1.2 value space (BCP-47, ASCII
//! lowercase) through [`purrdf_iri::langtag::identity_fold`] — the same fold
//! the upstream kernel interns under — so `en-US` and `en-us` convert to the
//! same value.

use crate::ast::{Graph, Literal, Name, Term, Triple};
use purrdf_rdf::{RdfLiteral, RdfTerm, RdfTriple, RdfTextDirection};

/// RDF syntax families accepted by the RDF compatibility layer (parser-facing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdfFormat {
    Turtle,
    NTriples,
    NQuads,
    Trig,
}

impl RdfFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "turtle" | "ttl" => Some(Self::Turtle),
            "n-triples" | "ntriples" | "nt" => Some(Self::NTriples),
            "n-quads" | "nquads" | "nq" => Some(Self::NQuads),
            "trig" => Some(Self::Trig),
            _ => None,
        }
    }
}

/// An N3 term/literal that cannot cross into the RDF term model losslessly.
///
/// Append-only: never renumber or rename existing variants; new lossy edges
/// get a new variant, never a reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionError {
    /// An upstream literal carries a base direction; the N3 literal model has
    /// no direction field, so admitting it would drop the direction.
    DirectionalLiteral {
        /// The literal's lexical form.
        lexical_form: String,
        /// The literal's language tag (already value-space folded).
        language: String,
        /// The literal's base direction (`ltr`/`rtl`).
        direction: String,
    },
    /// An upstream RDF 1.2 triple term (quoted triple) has no dedicated N3
    /// `Term` variant.
    TripleTerm,
    /// An N3 variable has no RDF term equivalent.
    Variable(String),
    /// A triple predicate must be an IRI; an N3 literal predicate has no RDF
    /// triple equivalent.
    LiteralPredicate,
    /// A blank-node predicate has no RDF triple equivalent (RDF predicates
    /// are IRIs).
    BlankPredicate,
    /// An N3 list has no RDF term equivalent (mapping it to an RDF collection
    /// would mint undeclared nodes).
    List(usize),
    /// An N3 formula has no RDF term equivalent.
    Formula,
}

impl core::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DirectionalLiteral {
                lexical_form,
                language,
                direction,
            } => write!(
                f,
                "directional language-tagged literal {lexical_form:?} (lang {language:?}, direction {direction}) has no N3 representation: the direction would be silently dropped"
            ),
            Self::TripleTerm => write!(
                f,
                "RDF 1.2 triple term has no dedicated N3 term variant; refusing rather than flattening into a formula"
            ),
            Self::Variable(name) => write!(
                f,
                "N3 variable ?{name} has no RDF term equivalent"
            ),
            Self::LiteralPredicate => write!(
                f,
                "N3 triple predicate is a literal; an RDF triple predicate must be an IRI"
            ),
            Self::BlankPredicate => write!(
                f,
                "N3 triple predicate is a blank node; an RDF triple predicate must be an IRI"
            ),
            Self::List(len) => write!(
                f,
                "N3 list of {len} items has no RDF term equivalent"
            ),
            Self::Formula => write!(
                f,
                "N3 formula has no RDF term equivalent"
            ),
        }
    }
}

impl std::error::Error for ConversionError {}

impl From<ConversionError> for crate::error::EyeronError {
    fn from(value: ConversionError) -> Self {
        crate::error::EyeronError::new(value.to_string())
    }
}

/// The upstream RDF term model, as this bridge's target.
pub type UpstreamTerm = RdfTerm;

/// Convert an N3 term into the upstream RDF term model.
///
/// Total: returns [`ConversionError`] for N3-only term kinds.
///
/// # Errors
/// [`ConversionError::Variable`], [`ConversionError::List`] or
/// [`ConversionError::Formula`] for N3-only term kinds.
pub fn term_to_rdf_term(term: &Term) -> Result<RdfTerm, ConversionError> {
    match term {
        Term::Iri(iri) => Ok(RdfTerm::iri(iri.as_str())),
        Term::Blank(label) => Ok(RdfTerm::blank_node(label.as_str())),
        Term::Literal(literal) => Ok(RdfTerm::literal(literal_to_rdf_literal(literal))),
        Term::Var(name) => Err(ConversionError::Variable(name.to_string())),
        Term::List(items) => Err(ConversionError::List(items.len())),
        Term::Formula(_) => Err(ConversionError::Formula),
    }
}

/// Convert an upstream RDF term into the N3 term model.
///
/// Total: returns [`ConversionError`] for RDF-only term kinds and for
/// directional literals.
///
/// # Errors
/// [`ConversionError::TripleTerm`] for an RDF 1.2 triple term;
/// [`ConversionError::DirectionalLiteral`] for a directional language-tagged
/// literal.
pub fn rdf_term_to_term(term: &RdfTerm) -> Result<Term, ConversionError> {
    match term {
        RdfTerm::Iri(iri) => Ok(Term::iri(iri.as_str())),
        RdfTerm::BlankNode(label) => Ok(Term::blank(label.as_str())),
        RdfTerm::Literal(literal) => Ok(Term::Literal(rdf_literal_to_literal(literal)?)),
        RdfTerm::Triple(_) => Err(ConversionError::TripleTerm),
    }
}

/// Convert an N3 triple into an upstream RDF triple (no source location).
///
/// # Errors
/// Same as [`term_to_rdf_term`], for any N3-only term in s/p/o.
pub fn triple_to_rdf_triple(triple: &Triple) -> Result<RdfTriple, ConversionError> {
    let Term::Iri(predicate) = &triple.p else {
        return Err(match &triple.p {
            Term::Literal(_) => ConversionError::LiteralPredicate,
            Term::Var(name) => ConversionError::Variable(name.to_string()),
            Term::List(items) => ConversionError::List(items.len()),
            Term::Formula(_) => ConversionError::Formula,
            // The `let`-`else` above already excluded `Iri`.
            Term::Iri(_) => unreachable!("IRI predicates convert"),
            Term::Blank(_) => ConversionError::BlankPredicate,
        });
    };
    Ok(RdfTriple::new(
        term_to_rdf_term(&triple.s)?,
        predicate.as_str(),
        term_to_rdf_term(&triple.o)?,
    ))
}

/// Convert an upstream RDF triple into an N3 triple. Source locations are
/// dropped at this boundary (the N3 [`crate::ast::Triple`] has no location).
///
/// # Errors
/// Same as [`rdf_term_to_term`], for any RDF-only term in s/p/o.
pub fn rdf_triple_to_triple(triple: RdfTriple) -> Result<Triple, ConversionError> {
    let (s, p, o, _location) = triple.into_parts();
    Ok(Triple::new(
        rdf_term_to_term(&s)?,
        Term::iri(p),
        rdf_term_to_term(&o)?,
    ))
}

/// Convert an N3 graph formula into upstream triples (asserted, unquoted).
///
/// Formula-scoped quantification is dropped at this boundary: a formula's
/// universals/existentials have no per-statement RDF equivalent. This is the
/// one documented boundary drop (asserting a formula is exactly how N3 says
/// "these statements hold"), and it is named in the function name.
///
/// # Errors
/// Same as [`triple_to_rdf_triple`], per triple.
pub fn formula_to_rdf_triples(graph: &Graph) -> Result<Vec<RdfTriple>, ConversionError> {
    graph.triples.iter().map(triple_to_rdf_triple).collect()
}

/// Convert an N3 literal into an upstream [`RdfLiteral`].
///
/// Lossless: lexical form, datatype IRI, and language tag all carry across.
/// The N3 literal model has no base direction, so the result never carries
/// one; the reverse conversion refuses directional literals (see
/// [`ConversionError::DirectionalLiteral`]).
#[must_use]
pub fn literal_to_rdf_literal(literal: &Literal) -> RdfLiteral {
    RdfLiteral {
        lexical_form: literal.value.to_string(),
        datatype: literal.datatype.as_ref().map(Name::to_string),
        language: literal.language.as_ref().map(|tag| {
            purrdf_iri::langtag::identity_fold(tag.as_str())
        }),
        direction: None,
    }
}

/// Convert an upstream [`RdfLiteral`] into an N3 literal.
///
/// Language tags are folded to the RDF 1.2 value space (ASCII lowercase) with
/// [`purrdf_iri::langtag::identity_fold`], so `en-US` and `en-us` arrive as
/// the same N3 literal. A datatype is preserved as authored (the upstream
/// model keeps the explicit field even for language-tagged literals, where
/// the intern-time implied datatype is derived, not stored).
///
/// # Errors
/// [`ConversionError::DirectionalLiteral`] — a base direction has no N3
/// representation; it is refused, never dropped.
pub fn rdf_literal_to_literal(literal: &RdfLiteral) -> Result<Literal, ConversionError> {
    if let Some(direction) = literal.direction {
        return Err(ConversionError::DirectionalLiteral {
            lexical_form: literal.lexical_form.clone(),
            language: literal
                .language
                .as_deref()
                .map_or_else(String::new, |tag| {
                    purrdf_iri::langtag::identity_fold(tag)
                }),
            direction: direction.as_str().to_string(),
        });
    }
    Ok(Literal {
        value: Name::from(literal.lexical_form.as_str()),
        datatype: literal.datatype.as_deref().map(Name::from),
        language: literal.language.as_deref().map(|tag| {
            Name::from(purrdf_iri::langtag::identity_fold(tag).as_str())
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(value: &str) -> Literal {
        Literal {
            value: Name::from(value),
            datatype: None,
            language: None,
        }
    }

    #[test]
    fn iri_blank_literal_roundtrip() {
        for term in [
            Term::iri("https://example.org/s"),
            Term::blank("b0"),
            Term::Literal(lit("hello")),
        ] {
            let up = term_to_rdf_term(&term).expect("lossless direction");
            let back = rdf_term_to_term(&up).expect("lossless direction");
            assert_eq!(term, back, "round-trip must be exact");
        }
    }

    #[test]
    fn datatyped_and_language_literals_roundtrip() {
        let typed = Literal {
            value: Name::from("42"),
            datatype: Some(Name::from("http://www.w3.org/2001/XMLSchema#integer")),
            language: None,
        };
        let up = term_to_rdf_term(&Term::Literal(typed.clone())).expect("typed literal converts");
        let RdfTerm::Literal(rdf_lit) = &up else {
            panic!("expected a literal term");
        };
        assert_eq!(rdf_lit.lexical_form, "42");
        assert_eq!(
            rdf_lit.datatype.as_deref(),
            Some("http://www.w3.org/2001/XMLSchema#integer")
        );
        assert_eq!(rdf_term_to_term(&up).expect("reverse"), Term::Literal(typed));

        // Language tags fold to the BCP-47 value space (ASCII lowercase).
        let tagged = Literal {
            value: Name::from("bonjour"),
            datatype: None,
            language: Some(Name::from("en-US")),
        };
        let up = term_to_rdf_term(&Term::Literal(tagged)).expect("language literal converts");
        let RdfTerm::Literal(rdf_lit) = &up else {
            panic!("expected a literal term");
        };
        assert_eq!(rdf_lit.language.as_deref(), Some("en-us"));
        assert!(rdf_lit.direction.is_none());
    }

    #[test]
    fn directional_literal_is_refused_not_dropped() {
        let directional = RdfLiteral {
            lexical_form: "مرحبا".into(),
            datatype: None,
            language: Some("ar".into()),
            direction: Some(RdfTextDirection::Rtl),
        };
        let error = rdf_term_to_term(&RdfTerm::literal(directional))
            .expect_err("a directional literal must be refused");
        assert!(
            matches!(error, ConversionError::DirectionalLiteral { .. }),
            "typed refusal required, got {error:?}"
        );
    }

    #[test]
    fn triple_terms_and_n3_only_kinds_are_typed_refusals() {
        assert!(matches!(
            rdf_term_to_term(&RdfTerm::triple(purrdf_rdf::RdfTriple::new(
                RdfTerm::iri("https://example.org/s"),
                "https://example.org/p",
                RdfTerm::iri("https://example.org/o"),
            ))),
            Err(ConversionError::TripleTerm)
        ));
        assert!(matches!(
            term_to_rdf_term(&Term::var("x")),
            Err(ConversionError::Variable(_))
        ));
        assert!(matches!(
            term_to_rdf_term(&Term::list(vec![Term::iri("https://example.org/a")])),
            Err(ConversionError::List(_))
        ));
        assert!(matches!(
            term_to_rdf_term(&Term::Formula(Graph::default())),
            Err(ConversionError::Formula)
        ));
    }

    #[test]
    fn triples_roundtrip_both_directions() {
        let n3_triple = Triple::new(
            Term::iri("https://example.org/s"),
            Term::iri("https://example.org/p"),
            Term::Literal(lit("v")),
        );
        let up = triple_to_rdf_triple(&n3_triple).expect("ground triple converts");
        assert_eq!(up.predicate, "https://example.org/p");
        let back = rdf_triple_to_triple(up).expect("reverse");
        assert_eq!(n3_triple, back);
    }

    #[test]
    fn formula_assertion_boundary() {
        let graph = Graph::from(vec![Triple::new(
            Term::iri("https://example.org/s"),
            Term::iri("https://example.org/p"),
            Term::Literal(lit("v")),
        )]);
        let triples = formula_to_rdf_triples(&graph).expect("ground formula asserts");
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].predicate, "https://example.org/p");
    }
}
