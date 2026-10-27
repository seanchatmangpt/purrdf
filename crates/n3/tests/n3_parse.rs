//! Parse-level integration courts for the native N3 parser (harness = false,
//! same binary runs through the workspace wasm runner).
//!
//! Chicago discipline: the real parser executes on real N3 text; assertions
//! land on the parsed `Document` (final state), never on call counts. Error
//! courts assert the concrete `EyeronError` fields (message content + byte
//! offset), not just `is_err`.

use purrdf_n3::ast::{Document, Term, Triple};
use purrdf_n3::parser::{parse_n3, parse_n3_with_source};

const BASE: &str = "http://example.org/base/";

fn ground_iri(t: &Term) -> Option<&str> {
    match t {
        Term::Iri(n) => Some(n.as_str()),
        _ => None,
    }
}

fn blank_label(t: &Term) -> Option<&str> {
    match t {
        Term::Blank(b) => Some(b.as_str()),
        _ => None,
    }
}

/// Court helper: require a typed refusal, returning (message, offset) for
/// assertion. Reports a FAIL string instead of panicking, so one bad surface
/// cannot abort the whole harness.
fn refuse(input: &str, base: Option<&str>, what: &str) -> Result<(String, Option<usize>), String> {
    match parse_n3(input, base) {
        Err(e) => Ok((e.message, e.offset)),
        Ok(doc) => Err(format!(
            "expected a refusal ({}), parsed Ok into {:?}",
            what, doc.facts
        )),
    }
}

fn expect(condition: bool, what: String) -> Result<(), String> {
    if condition { Ok(()) } else { Err(what) }
}

fn literal_of(t: &Term) -> Result<&purrdf_n3::ast::Literal, String> {
    match t {
        Term::Literal(l) => Ok(l),
        other => Err(format!("expected literal term, got {:?}", other)),
    }
}

fn formula_of<'a>(t: &'a Term) -> Result<&'a purrdf_n3::ast::Graph, String> {
    match t {
        Term::Formula(g) => Ok(g),
        other => Err(format!("expected formula term, got {:?}", other)),
    }
}

fn fact<'a>(
    doc: &'a Document,
    pred: impl Fn(&Triple) -> bool,
    what: &str,
) -> Result<&'a Triple, String> {
    doc.facts
        .iter()
        .find(|t| pred(t))
        .ok_or_else(|| format!("no fact matching {}: facts were {:?}", what, doc.facts))
}

// ---------------------------------------------------------------- prefixes

fn turtle_style_prefixes_resolve() -> Result<(), String> {
    let doc = parse_n3(
        "@prefix ex: <http://example.org/> .\nex:s ex:p ex:o .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        doc.prefixes.get("ex").map(String::as_str),
        Some("http://example.org/"),
        "@prefix must be registered on the document"
    );
    assert_eq!(doc.facts.len(), 1, "one fact expected, got {:?}", doc.facts);
    let t = &doc.facts[0];
    assert_eq!(ground_iri(&t.s), Some("http://example.org/s"));
    assert_eq!(ground_iri(&t.p), Some("http://example.org/p"));
    assert_eq!(ground_iri(&t.o), Some("http://example.org/o"));
    Ok(())
}

fn default_prefix_and_relative_iri_resolution() -> Result<(), String> {
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s :p <relative> .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let t = &doc.facts[0];
    assert_eq!(ground_iri(&t.s), Some("http://example.org/s"));
    assert_eq!(
        ground_iri(&t.o),
        Some("http://example.org/base/relative"),
        "relative IRI must resolve against the base IRI, not the default prefix"
    );
    Ok(())
}

// ---------------------------------------------------------------- literals

fn literals_with_lang_tag_and_datatype() -> Result<(), String> {
    let doc = parse_n3(
        concat!(
            "@prefix : <http://example.org/> .\n",
            "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n",
            ":x :label \"hello\"@en ;\n",
            "   :age \"42\"^^xsd:integer ;\n",
            "   :note \"plain\" .\n",
        ),
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let label = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/label"),
        "lang-tagged literal",
    )?;
    let l = literal_of(&label.o)?;
    assert_eq!(l.value.as_str(), "hello");
    assert_eq!(
        l.language.as_ref().map(|n| n.as_str()),
        Some("en"),
        "language tag must round-trip"
    );
    assert!(
        l.datatype.is_none(),
        "tagged literal carries no explicit datatype"
    );

    let age = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/age"),
        "typed literal",
    )?;
    let l = literal_of(&age.o)?;
    assert_eq!(l.value.as_str(), "42");
    let dt = l
        .datatype
        .as_ref()
        .map(|n| n.as_str())
        .expect("typed literal must carry a datatype");
    assert_eq!(dt, "http://www.w3.org/2001/XMLSchema#integer");

    let note = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/note"),
        "plain literal",
    )?;
    let l = literal_of(&note.o)?;
    assert_eq!(l.value.as_str(), "plain");
    assert!(l.language.is_none() && l.datatype.is_none());
    Ok(())
}

// ------------------------------------------------------- nested triple terms

fn nested_triple_term_becomes_formula_term() -> Result<(), String> {
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:c :states << :s :p :o >> .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let t = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/states"),
        "triple-term fact",
    )?;
    let g = formula_of(&t.o)?;
    assert_eq!(g.triples.len(), 1, "one nested triple expected");
    let inner = &g.triples[0];
    assert_eq!(ground_iri(&inner.s), Some("http://example.org/s"));
    assert_eq!(ground_iri(&inner.p), Some("http://example.org/p"));
    assert_eq!(ground_iri(&inner.o), Some("http://example.org/o"));
    Ok(())
}

fn parenthesized_triple_term_is_accepted() -> Result<(), String> {
    // `<<( :s :p :o )>>`: the inner parentheses are grouping around the
    // subject term, not a collection — the same nested triple results as the
    // unparenthesized spelling.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:c :states << ( :s :p :o ) >> .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let t = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/states"),
        "parenthesized triple-term fact",
    )?;
    let g = formula_of(&t.o)?;
    assert_eq!(g.triples.len(), 1, "one nested triple expected");
    let inner = &g.triples[0];
    assert_eq!(ground_iri(&inner.s), Some("http://example.org/s"));
    assert_eq!(ground_iri(&inner.p), Some("http://example.org/p"));
    assert_eq!(ground_iri(&inner.o), Some("http://example.org/o"));
    Ok(())
}

// ---------------------------------------------------------------- formulas

fn formula_context_as_term_and_rule() -> Result<(), String> {
    let doc = parse_n3(
        concat!(
            "@prefix : <http://example.org/> .\n",
            ":doc :mentions { :s :p :o . } .\n",
            "{ ?s :p ?o . } => { ?o :p ?s . } .\n",
        ),
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let mentions = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/mentions"),
        "formula-as-object fact",
    )?;
    let g = formula_of(&mentions.o)?;
    assert_eq!(g.triples.len(), 1, "formula context carries one triple");

    assert_eq!(
        doc.rules.len(),
        1,
        "the `{{ ... }} => {{ ... }}` statement must register one rule, got {:?}",
        doc.rules
    );
    let rule = &doc.rules[0];
    assert!(rule.is_forward, "=> is a forward rule");
    assert_eq!(rule.premise.triples.len(), 1);
    assert_eq!(rule.conclusion.triples.len(), 1);
    assert!(
        rule.premise.free_variables().iter().next().is_some(),
        "rule variables must remain free in the premise graph"
    );
    Ok(())
}

// --------------------------------------------------------------- @forAll

fn for_all_registers_root_universals() -> Result<(), String> {
    let doc = parse_n3(
        concat!(
            "@prefix : <http://example.org/> .\n",
            "@forAll :x, :y .\n",
            ":x :knows :y .\n",
        ),
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        doc.universals.len(),
        2,
        "universals were {:?}",
        doc.universals
    );
    assert!(
        doc.facts.len() >= 1,
        "the quantified statement must still parse as a fact"
    );
    Ok(())
}

fn for_some_registers_root_existentials() -> Result<(), String> {
    let doc = parse_n3(
        concat!(
            "@prefix : <http://example.org/> .\n",
            "@forSome :w .\n",
            ":a :met :w .\n",
        ),
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        doc.existentials.len(),
        1,
        "existentials were {:?}",
        doc.existentials
    );
    Ok(())
}

// ---------------------------------------------------------------- errors

fn error_unclosed_triple_term_names_the_construct_with_offset() -> Result<(), String> {
    let src = "@prefix : <http://example.org/> .\n:c :states << :s :p :o .\n";
    let err = parse_n3(src, Some(BASE)).expect_err("unclosed `<<` must be refused");
    assert!(
        err.message.to_lowercase().contains("<<") || err.message.to_lowercase().contains("triple"),
        "error message must name the construct, got: {}",
        err.message
    );
    assert!(
        err.offset.is_some(),
        "refusal must carry a byte offset, got {:?}",
        err.offset
    );
    Ok(())
}

fn error_unknown_prefix_is_refused_with_position() -> Result<(), String> {
    let err = parse_n3("nosuch:s nosuch:p nosuch:o .", Some(BASE))
        .expect_err("undeclared prefix must be refused");
    assert!(
        err.message.contains("nosuch"),
        "error must name the offending prefix, got: {}",
        err.message
    );
    assert!(err.offset.is_some(), "refusal must carry a byte offset");
    assert_eq!(err.offset, Some(0), "offense starts at byte 0");
    Ok(())
}

fn error_invalid_language_tag_is_refused() -> Result<(), String> {
    // Audit C8 (lane U1-SYNTAX F8): both profiles share one langtag grammar.
    // The N3 profile previously recorded tags verbatim, and this court pinned
    // that verbatim acceptance (`language == Some("e--n")`); it was rewritten
    // to a refusal court as part of the F8 fix receipt.
    let src = "@prefix : <http://example.org/> .\n:x :label \"hi\"@e--n .\n";
    let (message, offset) = refuse(src, Some(BASE), "malformed langtag must refuse on N3 too")?;
    expect(
        message.contains("language tag"),
        format!("error must name the construct, got: {}", message),
    )?;
    expect(
        offset == Some(src.find("@e--n").unwrap()),
        format!("refusal must point at the langtag itself, got {:?}", offset),
    )?;
    Ok(())
}

fn language_tag_case_folds_on_n3_profile() -> Result<(), String> {
    // Audit C8: `"v"@EN` and `"v"@en` must be the same literal, as on the RDF
    // 1.2 profile; the verbatim N3 arm made literal identity case-dependent.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:x :p \"v\"@EN ; :q \"v\"@en .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let mut tags: Vec<_> = doc
        .facts
        .iter()
        .filter_map(|t| match &t.o {
            Term::Literal(l) => l.language.as_ref().map(|n| n.clone()),
            _ => None,
        })
        .collect();
    tags.sort();
    tags.dedup();
    expect(
        tags == vec![purrdf_n3::ast::Name::from("en")],
        format!(
            "case-folded tags must collapse to one identity, got {:?}",
            tags
        ),
    )
}

fn error_dangling_statement_is_refused() -> Result<(), String> {
    // A subject/predicate with no object before EOF.
    let src = "@prefix : <http://example.org/> .\n:s :p\n";
    let err = parse_n3(src, Some(BASE)).expect_err("statement without an object must be refused");
    assert!(
        err.offset.is_some() && err.offset.unwrap() >= src.len() - 2,
        "refusal should point at/near EOF ({}), got {:?}",
        src.len(),
        err.offset
    );
    Ok(())
}

fn source_label_lands_in_rendered_diagnostic() -> Result<(), String> {
    let err = parse_n3_with_source("<< :s :p", None, Some("broken.n3"))
        .expect_err("truncated document must be refused");
    let rendered = err.with_source_location("<< :s :p", "broken.n3");
    assert!(
        rendered.starts_with("broken.n3:"),
        "rendered diagnostic must be label-anchored, got: {}",
        rendered
    );
    assert!(
        rendered.contains('^'),
        "rendered diagnostic must include a caret line"
    );
    Ok(())
}

// ------------------------------------------- F2 verb-path chain triples

fn verb_path_chain_triples_are_kept_forward() -> Result<(), String> {
    // Audit C7 (lane U1-SYNTAX F2): `parse_verb` bound `(term, _generated)`
    // and returned only the term, so `:s :a!:b :o .` asserted `:s _:pathN :o`
    // with the defining chain `:a :b _:pathN` silently discarded.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s :a!:b :o .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let chain = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/a")
                && ground_iri(&t.p) == Some("http://example.org/b")
        },
        "chain triple `:a :b _:pathN`",
    )?;
    let path_label = match &chain.o {
        Term::Blank(b) => b.as_str().to_string(),
        other => {
            return Err(format!(
                "chain object must be the fresh path node, got {:?}",
                other
            ));
        }
    };
    let asserted = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/s")
                && ground_iri(&t.o) == Some("http://example.org/o")
        },
        "asserted triple `:s _:pathN :o`",
    )?;
    match &asserted.p {
        Term::Blank(b) if b.as_str() == path_label => Ok(()),
        other => Err(format!(
            "predicate must be the same path node {}, got {:?}",
            path_label, other
        )),
    }
}

fn verb_path_chain_triples_are_kept_reverse() -> Result<(), String> {
    // The `^` operator's chain orientation is `_:pathN :b :a` (the path node
    // points back at the start), and it must survive verb position too.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s :a^:b :o .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let chain = fact(
        &doc,
        |t| {
            blank_label(&t.s).is_some()
                && ground_iri(&t.p) == Some("http://example.org/b")
                && ground_iri(&t.o) == Some("http://example.org/a")
        },
        "reverse chain triple `_:pathN :b :a`",
    )?;
    let path_label = match &chain.s {
        Term::Blank(b) => b.as_str().to_string(),
        other => {
            return Err(format!(
                "chain subject must be the fresh path node, got {:?}",
                other
            ));
        }
    };
    let asserted = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/s")
                && ground_iri(&t.o) == Some("http://example.org/o")
        },
        "asserted triple `:s _:pathN :o`",
    )?;
    match &asserted.p {
        Term::Blank(b) if b.as_str() == path_label => Ok(()),
        other => Err(format!(
            "predicate must be the same path node {}, got {:?}",
            path_label, other
        )),
    }
}

fn verb_path_chains_survive_verb_lists() -> Result<(), String> {
    // Semicolon verb groups each parse their own verb; every group's chain
    // must be kept, not just the first.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s :a!:b :o ; :c!:d :e .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    for (a, b) in [("a", "b"), ("c", "d")] {
        let chain = fact(
            &doc,
            |t| {
                ground_iri(&t.s) == Some(&format!("http://example.org/{}", a))
                    && ground_iri(&t.p) == Some(&format!("http://example.org/{}", b))
            },
            &format!("chain triple `:{} :{} _:pathN`", a, b),
        )?;
        match &chain.o {
            Term::Blank(_) => {}
            other => {
                return Err(format!(
                    "chain object must be the fresh path node, got {:?}",
                    other
                ));
            }
        }
    }
    Ok(())
}

fn is_of_path_keeps_chain_and_orientation() -> Result<(), String> {
    // `S is <path> of O` asserts (O, path, S); the path's chain triples must
    // survive alongside that flipped orientation.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s is :a!:b of :o .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let chain = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/a")
                && ground_iri(&t.p) == Some("http://example.org/b")
        },
        "chain triple `:a :b _:pathN`",
    )?;
    let path_label = match &chain.o {
        Term::Blank(b) => b.as_str().to_string(),
        other => {
            return Err(format!(
                "chain object must be the fresh path node, got {:?}",
                other
            ));
        }
    };
    let asserted = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/o")
                && ground_iri(&t.o) == Some("http://example.org/s")
        },
        "asserted triple `:o _:pathN :s`",
    )?;
    match &asserted.p {
        Term::Blank(b) if b.as_str() == path_label => Ok(()),
        other => Err(format!(
            "predicate must be the same path node {}, got {:?}",
            path_label, other
        )),
    }
}

fn triple_term_predicate_path_chain_is_kept() -> Result<(), String> {
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:c :states << :s :a!:b :o >> .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let chain = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/a")
                && ground_iri(&t.p) == Some("http://example.org/b")
        },
        "chain triple `:a :b _:pathN`",
    )?;
    let path_label = match &chain.o {
        Term::Blank(b) => b.as_str().to_string(),
        other => {
            return Err(format!(
                "chain object must be the fresh path node, got {:?}",
                other
            ));
        }
    };
    let nested = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/states"),
        "triple-term fact",
    )?;
    let g = formula_of(&nested.o)?;
    expect(
        g.triples.len() == 1,
        format!("one nested triple expected, got {:?}", g.triples),
    )?;
    match &g.triples[0].p {
        Term::Blank(b) if b.as_str() == path_label => Ok(()),
        other => Err(format!(
            "nested predicate must be the same path node {}, got {:?}",
            path_label, other
        )),
    }
}

// ------------------------------------------------- F3 bare-word fabrication

fn error_bare_word_is_refused_not_fabricated() -> Result<(), String> {
    // Audit C4 (lane U1-SYNTAX F3): a colon-less word used to fall through
    // expand_pname's tail to `http://example.org/{}` — fabricated silently.
    let src = "nope nope nope .";
    let (message, offset) = refuse(
        src,
        Some(BASE),
        "bare word must refuse, not mint example.org",
    )?;
    expect(
        message.contains("nope"),
        format!("error must name the construct, got: {}", message),
    )?;
    expect(
        offset == Some(0),
        format!("offense starts at byte 0, got {:?}", offset),
    )?;
    Ok(())
}

fn error_bare_word_datatype_is_refused() -> Result<(), String> {
    let src = "@prefix : <http://example.org/> .\n:x :p \"1\"^^foo .\n";
    let (message, offset) = refuse(src, Some(BASE), "bare datatype word must refuse")?;
    expect(
        message.contains("foo"),
        format!("error must name the construct, got: {}", message),
    )?;
    expect(
        offset == Some(src.find("foo").unwrap()),
        format!("refusal must point at the bare word, got {:?}", offset),
    )?;
    Ok(())
}

// ------------------------------------------ F7 eager @prefix split (lexer)

fn error_at_prefix_suffix_word_is_refused() -> Result<(), String> {
    // Audit C11 (lane U1-SYNTAX F7): the lexer split any `@prefix*` word into
    // `AtPrefix` + `PName(suffix)`, so `@prefixfr: <http://x/> .` tokenized
    // into a complete (invalid) prefix declaration that parse_prefix consumed.
    let src = "@prefixfr: <http://x/> .\n";
    let (message, offset) = refuse(src, None, "@prefix-suffixed word is not a directive")?;
    expect(
        message.contains("prefixfr"),
        format!("error must name the construct, got: {}", message),
    )?;
    expect(
        offset == Some(0),
        format!("offense starts at byte 0, got {:?}", offset),
    )?;
    Ok(())
}

fn langtag_prefix_word_keeps_its_language() -> Result<(), String> {
    // `"lit"@prefixfr` must reach the Lang arm with its tag intact (the eager
    // split used to steal it into `AtPrefix` + `PName("fr")` and error at a
    // shifted offset).  The tag `prefixfr` is a conforming tag shape, so the
    // literal keeps language `prefixfr`.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:x :p \"lit\"@prefixfr .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let lit = fact(
        &doc,
        |t| ground_iri(&t.p) == Some("http://example.org/p"),
        "langtag fact",
    )?;
    let l = literal_of(&lit.o)?;
    expect(
        l.language.as_ref().map(|n| n.as_str()) == Some("prefixfr"),
        format!(
            "the language must survive the lexer's @-word dispatch, got {:?}",
            l.language
        ),
    )
}

// ------------------------------------------- F9 exponentless number literal

fn error_number_without_exponent_digit_is_refused() -> Result<(), String> {
    // Audit wave-2 surface (lane U1-SYNTAX F9): `1e` / `1e+` lexed as Number
    // with zero exponent digits and the N3 route minted `"1e"^^xsd:double`.
    for tail in ["1e", "1e+", "1e-"] {
        let src = format!("@prefix : <http://example.org/> .\n:x :p {} .\n", tail);
        let (message, offset) = refuse(
            &src,
            Some(BASE),
            &format!("`{}` has no exponent digit and must be refused", tail),
        )?;
        expect(
            message.contains("exponent"),
            format!("error must name the construct, got: {}", message),
        )?;
        let want = src.find(tail).unwrap();
        expect(
            offset == Some(want),
            format!("refusal must point at the number, got {:?}", offset),
        )?;
    }
    // Conforming exponents keep parsing, with the same lexical form.
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:x :p 1e5 .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    expect(
        doc.facts.len() == 1,
        format!("one fact expected, got {:?}", doc.facts),
    )?;
    let lit = literal_of(&doc.facts[0].o)?;
    expect(
        lit.value.as_str() == "1e5",
        format!("lexical form preserved, got {:?}", lit.value),
    )?;
    expect(
        lit.datatype.as_ref().map(|n| n.as_str())
            == Some("http://www.w3.org/2001/XMLSchema#double"),
        format!("exponent literal is xsd:double, got {:?}", lit.datatype),
    )?;
    Ok(())
}

// ------------------------------------------ F10 spaced N3 path operators

fn spaced_path_operators_parse_like_unspaced() -> Result<(), String> {
    // Audit wave-2 surface (lane U1-SYNTAX F10): `:p1 ! :p2` (whitespace
    // around a path operator) is the same path as `:p1!:p2`; the standalone
    // operator word used to die in split_unescaped_path.
    let src = "@prefix : <http://example.org/> .\n";
    let spaced =
        parse_n3(&format!("{}\n:s :p1 ! :p2 :o .\n", src), None).map_err(|e| e.to_string())?;
    let unspaced =
        parse_n3(&format!("{}\n:s :p1!:p2 :o .\n", src), None).map_err(|e| e.to_string())?;
    expect(
        spaced.facts == unspaced.facts,
        format!(
            "spaced and unspaced spellings must produce identical facts;\nspaced:   {:?}\nunspaced: {:?}",
            spaced.facts, unspaced.facts
        ),
    )?;
    expect(
        spaced.facts.len() == 2,
        format!("chain + asserted expected, got {:?}", spaced.facts),
    )?;
    Ok(())
}

fn spaced_reverse_path_operator_orients_the_chain_backwards() -> Result<(), String> {
    let doc = parse_n3(
        "@prefix : <http://example.org/> .\n:s :p :o ^ :q .\n",
        Some(BASE),
    )
    .map_err(|e| e.to_string())?;
    let chain = fact(
        &doc,
        |t| {
            blank_label(&t.s).is_some()
                && ground_iri(&t.p) == Some("http://example.org/q")
                && ground_iri(&t.o) == Some("http://example.org/o")
        },
        "reverse chain `_:pathN :q :o`",
    )?;
    let path_label = match &chain.s {
        Term::Blank(b) => b.as_str().to_string(),
        other => {
            return Err(format!(
                "chain subject must be the fresh path node, got {:?}",
                other
            ));
        }
    };
    let asserted = fact(
        &doc,
        |t| {
            ground_iri(&t.s) == Some("http://example.org/s")
                && ground_iri(&t.p) == Some("http://example.org/p")
        },
        "asserted triple `:s :p _:pathN`",
    )?;
    match &asserted.o {
        Term::Blank(b) if b.as_str() == path_label => Ok(()),
        other => Err(format!(
            "object must be the same path node {}, got {:?}",
            path_label, other
        )),
    }
}

fn spaced_path_composite_item_splits_correctly() -> Result<(), String> {
    // The item after a spaced operator may carry further unescaped operators
    // (`:p1 ! :p2^:p3` == `:p1!:p2^:p3`); the spaced operator orients the
    // item's first component, later components keep their own operators.
    let src = "@prefix : <http://example.org/> .\n";
    let spaced =
        parse_n3(&format!("{}\n:s :p1 ! :p2^:p3 :o .\n", src), None).map_err(|e| e.to_string())?;
    let unspaced =
        parse_n3(&format!("{}\n:s :p1!:p2^:p3 :o .\n", src), None).map_err(|e| e.to_string())?;
    expect(
        spaced.facts == unspaced.facts,
        format!(
            "composite spaced item must equal the unspaced path;\nspaced:   {:?}\nunspaced: {:?}",
            spaced.facts, unspaced.facts
        ),
    )?;
    expect(
        spaced.facts.len() == 3,
        format!(
            "two chain steps + asserted expected, got {:?}",
            spaced.facts
        ),
    )
}

fn error_leading_path_operator_still_refused() -> Result<(), String> {
    // A path needs a leading item; a spaced operator with nothing before it
    // keeps its refusal (the continuation only extends an existing path).
    let src = "@prefix : <http://example.org/> .\n! :p :q :r .\n";
    let (message, _offset) = refuse(src, Some(BASE), "a path needs a leading item")?;
    expect(
        message.contains("path component"),
        format!("error must name the construct, got: {}", message),
    )
}

fn main() {
    let cases: &[(&str, fn() -> Result<(), String>)] = &[
        (
            "turtle_style_prefixes_resolve",
            turtle_style_prefixes_resolve,
        ),
        (
            "default_prefix_and_relative_iri_resolution",
            default_prefix_and_relative_iri_resolution,
        ),
        (
            "literals_with_lang_tag_and_datatype",
            literals_with_lang_tag_and_datatype,
        ),
        (
            "nested_triple_term_becomes_formula_term",
            nested_triple_term_becomes_formula_term,
        ),
        (
            "parenthesized_triple_term_is_accepted",
            parenthesized_triple_term_is_accepted,
        ),
        (
            "formula_context_as_term_and_rule",
            formula_context_as_term_and_rule,
        ),
        (
            "for_all_registers_root_universals",
            for_all_registers_root_universals,
        ),
        (
            "for_some_registers_root_existentials",
            for_some_registers_root_existentials,
        ),
        (
            "error_unclosed_triple_term_names_the_construct_with_offset",
            error_unclosed_triple_term_names_the_construct_with_offset,
        ),
        (
            "error_unknown_prefix_is_refused_with_position",
            error_unknown_prefix_is_refused_with_position,
        ),
        (
            "error_invalid_language_tag_is_refused",
            error_invalid_language_tag_is_refused,
        ),
        (
            "error_dangling_statement_is_refused",
            error_dangling_statement_is_refused,
        ),
        (
            "source_label_lands_in_rendered_diagnostic",
            source_label_lands_in_rendered_diagnostic,
        ),
        (
            "language_tag_case_folds_on_n3_profile",
            language_tag_case_folds_on_n3_profile,
        ),
        (
            "verb_path_chain_triples_are_kept_forward",
            verb_path_chain_triples_are_kept_forward,
        ),
        (
            "verb_path_chain_triples_are_kept_reverse",
            verb_path_chain_triples_are_kept_reverse,
        ),
        (
            "verb_path_chains_survive_verb_lists",
            verb_path_chains_survive_verb_lists,
        ),
        (
            "is_of_path_keeps_chain_and_orientation",
            is_of_path_keeps_chain_and_orientation,
        ),
        (
            "triple_term_predicate_path_chain_is_kept",
            triple_term_predicate_path_chain_is_kept,
        ),
        (
            "error_bare_word_is_refused_not_fabricated",
            error_bare_word_is_refused_not_fabricated,
        ),
        (
            "error_bare_word_datatype_is_refused",
            error_bare_word_datatype_is_refused,
        ),
        (
            "error_at_prefix_suffix_word_is_refused",
            error_at_prefix_suffix_word_is_refused,
        ),
        (
            "langtag_prefix_word_keeps_its_language",
            langtag_prefix_word_keeps_its_language,
        ),
        (
            "error_number_without_exponent_digit_is_refused",
            error_number_without_exponent_digit_is_refused,
        ),
        (
            "spaced_path_operators_parse_like_unspaced",
            spaced_path_operators_parse_like_unspaced,
        ),
        (
            "spaced_reverse_path_operator_orients_the_chain_backwards",
            spaced_reverse_path_operator_orients_the_chain_backwards,
        ),
        (
            "spaced_path_composite_item_splits_correctly",
            spaced_path_composite_item_splits_correctly,
        ),
        (
            "error_leading_path_operator_still_refused",
            error_leading_path_operator_still_refused,
        ),
    ];
    let mut passed = 0usize;
    let mut failed = 0usize;
    for (name, case) in cases {
        match case() {
            Ok(()) => {
                passed += 1;
                println!("PASS {}", name);
            }
            Err(reason) => {
                failed += 1;
                println!("FAIL {}: {}", name, reason);
            }
        }
    }
    println!("{} passed, {} failed (of {})", passed, failed, cases.len());
    if failed > 0 {
        std::process::exit(1);
    }
}
