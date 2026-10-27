// SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Wasm surface for the [`crate`] N3 engine, ported from graphlaw-eyeron's
//! `src/wasm.rs` onto purrdf's conventions (see `crates/rdf-wasm`):
//!
//! - `wasm-bindgen` with camelCase `js_name` exports and
//!   `Result<String, JsValue>` for recoverable errors; a structured JSON
//!   error envelope for the report variants (same shape eyeron shipped).
//! - A `version()` liveness probe (`purrdf`'s wasm entry convention).
//! - No allocator swap (upstream does not use `wee_alloc`) and no
//!   `#[wasm_bindgen(start)]` setup needed: the N3 engine keeps no global
//!   state, so there is no stack floor to install and no panic poisoner to
//!   arm here — those are `crates/rdf-wasm` concerns, not this module's.
//!
//! Wiring (coordinator): in `src/lib.rs`
//! `#[cfg(feature = "wasm")] pub mod wasm;` plus, in `Cargo.toml`, a `wasm`
//! feature that enables the `wasm-bindgen` dependency and `crate-type =
//! ["rlib", "cdylib"]` for the wasm32 build.

#![allow(clippy::inline_always)]

use purrdf_lex::json_escape::{JsonEscapes, push_string};
use wasm_bindgen::prelude::*;

use crate::ast::Document;
use crate::error::{EyeronError, Result};
use crate::parser::{
    is_rdf_message_log, parse_n3, parse_n3_with_source, parse_rdf_message_log, parse_rdf12_compat,
};
use crate::printing::{rdf_result_to_string, result_to_string};
use crate::proof_writer::proof_to_n3;
use crate::rdf_compat::RdfFormat;
use crate::reasoner::{
    PreparedReasoner, ReasonerError, ReasonerOptions, ReasonerResult, reason as reason_document,
};

#[wasm_bindgen(js_name = version)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(text: string) => void")]
    pub type OutputCallback;

    #[wasm_bindgen(method, catch, js_name = call)]
    fn send(
        this: &OutputCallback,
        context: &JsValue,
        text: &str,
    ) -> std::result::Result<JsValue, JsValue>;
}

#[wasm_bindgen(js_name = reasonWithOptions)]
pub fn reason_with_options(
    input: &str,
    proof: bool,
    rdf: bool,
    rdf_format: &str,
) -> std::result::Result<String, JsValue> {
    run(input, "", proof, rdf, rdf_format).map_err(|err| JsValue::from_str(&err.to_string()))
}

#[wasm_bindgen(js_name = reason)]
pub fn reason(input: &str) -> std::result::Result<String, JsValue> {
    run(input, "", false, false, "n3").map_err(|err| JsValue::from_str(&err.to_string()))
}

#[wasm_bindgen(js_name = reasonWithData)]
pub fn reason_with_data(
    program: &str,
    data: &str,
    proof: bool,
    rdf: bool,
    rdf_format: &str,
) -> std::result::Result<String, JsValue> {
    run(program, data, proof, rdf, rdf_format).map_err(|err| JsValue::from_str(&err.to_string()))
}

#[wasm_bindgen(js_name = reasonWithDataReport)]
pub fn reason_with_data_report(
    program: &str,
    data: &str,
    proof: bool,
    rdf: bool,
    rdf_format: &str,
) -> String {
    match run_report(program, data, proof, rdf, rdf_format) {
        Ok(output) => format!("{{\"ok\":true,\"output\":{}}}", json_string(&output)),
        Err(err) => err.to_json(),
    }
}

/// A reusable Wasm reasoner. The N3 program is parsed and its forward-rule
/// agenda is built once in the constructor; each call receives a fresh,
/// independent RDF data batch.
#[wasm_bindgen(js_name = N3Session)]
pub struct N3Session {
    prepared: PreparedReasoner,
    proof: bool,
}

#[wasm_bindgen(js_class = N3Session)]
impl N3Session {
    #[wasm_bindgen(constructor)]
    pub fn new(program: &str, proof: bool) -> std::result::Result<N3Session, JsValue> {
        let doc = parse_source(program, proof, false, "n3", "session-program")
            .map_err(|err| JsValue::from_str(&err.with_source_location(program, "program")))?;
        Ok(Self {
            prepared: PreparedReasoner::new(doc),
            proof,
        })
    }

    /// Reason over a single independent data batch.
    pub fn reason(
        &self,
        data: &str,
        rdf: bool,
        rdf_format: &str,
    ) -> std::result::Result<String, JsValue> {
        self.run(data, rdf, rdf_format)
            .map(|run| run.output)
            .map_err(|err| JsValue::from_str(&err.display))
    }

    /// Like `reason`, but returns the same structured JSON error envelope as
    /// `reasonWithDataReport`, plus per-run reasoner statistics.
    #[wasm_bindgen(js_name = reasonReport)]
    pub fn reason_report(&self, data: &str, rdf: bool, rdf_format: &str) -> String {
        match self.run(data, rdf, rdf_format) {
            Ok(run) => format!(
                "{{\"ok\":true,\"output\":{},\"statistics\":{{\"iterations\":{},\"matchSteps\":{},\"explicitFacts\":{},\"derivedFacts\":{},\"rules\":{}}}}}",
                json_string(&run.output),
                run.result.statistics.iterations,
                run.result.statistics.match_steps,
                run.result.explicit.len(),
                run.result.derived.len(),
                run.result.rules.len(),
            ),
            Err(err) => err.to_json(),
        }
    }

    #[wasm_bindgen(getter, js_name = programRules)]
    pub fn program_rules(&self) -> usize {
        self.prepared.program().rules.len()
    }

    #[wasm_bindgen(getter, js_name = programFacts)]
    pub fn program_facts(&self) -> usize {
        self.prepared.program().facts.len()
    }
}

struct SessionRun {
    output: String,
    result: ReasonerResult,
}

impl N3Session {
    fn run(
        &self,
        data: &str,
        rdf: bool,
        rdf_format: &str,
    ) -> std::result::Result<SessionRun, PlaygroundError> {
        let data_doc = if data.trim().is_empty() {
            Document::new()
        } else {
            parse_source(data, false, rdf, rdf_format, "session-data")
                .map_err(|err| PlaygroundError::from_error(err, data, "data", "session-data"))?
        };
        let mut prefixes = data_doc.prefixes.clone();
        prefixes.extend(self.prepared.program().prefixes.clone());
        let result = self.prepared.reason(
            &data_doc,
            &ReasonerOptions {
                proof: self.proof,
                ..ReasonerOptions::default()
            },
        );
        if !result.is_complete() {
            return Err(PlaygroundError::from_reasoner(&result));
        }
        let output = if self.proof {
            proof_to_n3(&prefixes, &result)
        } else if rdf {
            rdf_result_to_string(&prefixes, &result.derived)
        } else {
            result_to_string(&prefixes, &result.derived)
        };
        Ok(SessionRun { output, result })
    }
}

fn run(program: &str, data: &str, proof: bool, rdf: bool, rdf_format: &str) -> Result<String> {
    run_report(program, data, proof, rdf, rdf_format).map_err(|err| EyeronError::new(err.display))
}

fn run_report(
    program: &str,
    data: &str,
    proof: bool,
    rdf: bool,
    rdf_format: &str,
) -> std::result::Result<String, PlaygroundError> {
    let mut doc = Document::new();
    if data.trim().is_empty() {
        let parsed = parse_source(program, proof, rdf, rdf_format, "playground")
            .map_err(|err| PlaygroundError::from_error(err, program, "program", "playground"))?;
        doc.merge(parsed);
    } else {
        let data_doc = parse_source(data, false, rdf, rdf_format, "playground-data")
            .map_err(|err| PlaygroundError::from_error(err, data, "data", "playground-data"))?;
        doc.merge(data_doc);
        let program_doc = parse_source(program, proof, false, "n3", "playground")
            .map_err(|err| PlaygroundError::from_error(err, program, "program", "playground"))?;
        doc.merge(program_doc);
    }

    let result = reason_document(
        &doc,
        &ReasonerOptions {
            proof,
            ..ReasonerOptions::default()
        },
    );
    if !result.is_complete() {
        return Err(PlaygroundError::from_reasoner(&result));
    }
    if proof {
        Ok(proof_to_n3(&doc.prefixes, &result))
    } else if rdf {
        Ok(rdf_result_to_string(&doc.prefixes, &result.derived))
    } else {
        Ok(result_to_string(&doc.prefixes, &result.derived))
    }
}

#[derive(Debug)]
struct PlaygroundError {
    code: &'static str,
    display: String,
    message: String,
    editor: &'static str,
    line: Option<usize>,
    column: Option<usize>,
    details: Option<String>,
}

impl PlaygroundError {
    fn from_error(err: EyeronError, source: &str, editor: &'static str, label: &str) -> Self {
        let (line, column) = err
            .offset
            .map(|offset| line_col(source, offset))
            .unwrap_or((0, 0));
        Self {
            code: "parse_error",
            display: err.with_source_location(source, label),
            message: err.message,
            editor,
            line: (line > 0).then_some(line),
            column: (column > 0).then_some(column),
            details: None,
        }
    }

    fn from_reasoner(result: &ReasonerResult) -> Self {
        let message = result
            .incomplete_summary()
            .unwrap_or_else(|| "reasoning incomplete".to_string());
        let limits = result
            .limits_reached
            .iter()
            .map(|limit| json_string(&limit.to_string()))
            .collect::<Vec<_>>()
            .join(",");
        let errors = result
            .errors
            .iter()
            .map(|error| match error {
                ReasonerError::UnsupportedBuiltin {
                    builtin, detail, ..
                } => format!(
                    "{{\"code\":\"unsupported_builtin\",\"builtin\":{},\"detail\":{}}}",
                    json_string(builtin),
                    json_string(detail),
                ),
            })
            .collect::<Vec<_>>()
            .join(",");
        Self {
            code: "reasoning_incomplete",
            display: message.clone(),
            message,
            editor: "program",
            line: None,
            column: None,
            details: Some(format!(
                "{{\"limitsReached\":[{}],\"errors\":[{}]}}",
                limits, errors
            )),
        }
    }

    fn to_json(&self) -> String {
        let line = self
            .line
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let column = self
            .column
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let details = self.details.as_deref().unwrap_or("null");
        format!(
            "{{\"ok\":false,\"error\":{{\"code\":{},\"message\":{},\"display\":{},\"editor\":{},\"line\":{},\"column\":{},\"details\":{}}}}}",
            json_string(self.code),
            json_string(&self.message),
            json_string(&self.display),
            json_string(self.editor),
            line,
            column,
            details,
        )
    }
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// JSON string escaper — the shared `purrdf_lex::json_escape` law
/// (`push_string` with `JsonEscapes::Minimal`), not a local implementation.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    purrdf_lex::json_escape::push_string(&mut out, value, JsonEscapes::Minimal);
    out
}

fn parse_source(
    input: &str,
    proof: bool,
    rdf: bool,
    rdf_format: &str,
    label: &str,
) -> Result<Document> {
    if is_rdf_message_log(input) {
        parse_rdf_message_log(input, None)
    } else if rdf {
        let format = rdf_format_from_playground(rdf_format, input)?;
        parse_rdf12_compat(input, None, format)
    } else if proof {
        parse_n3_with_source(input, None, Some(label))
    } else {
        parse_n3(input, None)
    }
}

fn rdf_format_from_playground(format: &str, input: &str) -> Result<RdfFormat> {
    match format.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(guess_rdf_format(input)),
        "ttl" | "turtle" => Ok(RdfFormat::Turtle),
        "trig" => Ok(RdfFormat::Trig),
        "nt" | "n-triples" | "ntriples" => Ok(RdfFormat::NTriples),
        "nq" | "n-quads" | "nquads" => Ok(RdfFormat::NQuads),
        other => Err(EyeronError::new(format!(
            "unknown RDF format for playground: {}",
            other
        ))),
    }
}

fn guess_rdf_format(input: &str) -> RdfFormat {
    let trimmed = input.trim_start();
    if trimmed.lines().all(|line| {
        let line = line.trim();
        line.is_empty()
            || line.starts_with('#')
            || (line.starts_with('<') && line.ends_with(" .") && line.matches('<').count() >= 4)
    }) {
        return RdfFormat::NTriples;
    }

    if looks_like_trig(trimmed) {
        RdfFormat::Trig
    } else {
        RdfFormat::Turtle
    }
}

fn looks_like_trig(input: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut prev_non_ws: Option<char> = None;
    for ch in input.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        match ch {
            '{' if depth == 0 => {
                if !matches!(prev_non_ws, Some('=') | Some('>') | Some('<')) {
                    return true;
                }
                depth += 1;
            }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if !ch.is_whitespace() {
            prev_non_ws = Some(ch);
        }
    }
    false
}
