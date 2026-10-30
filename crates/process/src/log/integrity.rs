// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Canonical identifiers and the hash-chained integrity ledger.
//!
//! Provenance: deterministic id minting generalizes ash_ex4pm's
//! `event_id/4`; the chain law follows beam4pm `zcode_ocel_verifier.ex`
//! (`pi_hash = H(parent ++ "\n" ++ canon)`, `pi_parent` = previous hash)
//! with BLAKE3 in place of sha256 and canonical JSON from
//! `purrdf_lex::json`.

use purrdf_hash::blake3;
use purrdf_hash::hex;
use purrdf_lex::json::{Object as Json, Value, write_compact};

use crate::error::ProcessError;
use crate::log::model::{AttrValue, EventObjectRef, OcelEvent, OcelLog};

/// The chain canonicalization of one event: fixed fields only
/// (`id`, `relationships`, `time`, `type`), in that key order — beam4pm's
/// `canon/3`, so a stamped `pi_hash` attribute never feeds its own chain.
#[must_use]
pub fn canonical_event_json(event: &OcelEvent) -> String {
    write_compact(&canonical_event_value(event))
}

/// The full-identity canonicalization used for id minting: fixed fields
/// plus name-ordered attributes, so two events differing only in attributes
/// still mint distinct ids.
#[must_use]
pub fn canonical_event_id(event: &OcelEvent) -> String {
    let mut root = Json::new();
    root.push(
        "id",
        event
            .id
            .clone()
            .map_or(Value::Null, |id| Value::from(id.as_str())),
    );
    root.push("time", Value::from(event.time.as_str()));
    root.push("type", Value::from(event.event_type.as_str()));
    let mut attributes = Json::new();
    for (name, item) in &event.attributes {
        attributes.push(name.as_str(), attr_to_value(item));
    }
    root.push("attributes", Value::Object(attributes));
    let mut sorted: Vec<&EventObjectRef> = event.relationships.iter().collect();
    sorted.sort_by(|a, b| (&a.object_id, &a.qualifier).cmp(&(&b.object_id, &b.qualifier)));
    let refs: Vec<Value> = sorted.iter().map(|reference| ref_json(reference)).collect();
    root.push("relationships", Value::Array(refs));
    let digest = blake3::hash(write_compact(&Value::Object(root)).as_bytes());
    short_digest(&digest)
}

/// One reference as a JSON value (id + qualifier, key order fixed).
fn ref_json(reference: &EventObjectRef) -> Value {
    let mut pair = Json::new();
    pair.push("objectId", Value::from(reference.object_id.as_str()));
    pair.push("qualifier", Value::from(reference.qualifier.as_str()));
    Value::Object(pair)
}

/// One event attribute's text payload.
fn attr_text(event: &OcelEvent, name: &str) -> Option<String> {
    match event.attributes.get(name) {
        Some(AttrValue::Text(text)) => Some(text.clone()),
        _ => None,
    }
}

/// Stamp `pi_hash`/`pi_parent` onto every event (canonical order) and
/// return the chain head.
pub fn stamp_pi_chain(log: &mut OcelLog) -> String {
    let mut events = log.sorted_events();
    let mut parent = String::new();
    for event in &mut events {
        let canonical = canonical_event_json(event);
        let input = format!("{parent}\n{canonical}");
        let digest = hex::encode(blake3::hash(input.as_bytes()).as_bytes());
        event
            .attributes
            .insert("pi_hash".to_owned(), AttrValue::Text(digest.clone()));
        event
            .attributes
            .insert("pi_parent".to_owned(), AttrValue::Text(parent.clone()));
        parent = digest;
    }
    // The stamped copy replaces the log's events, now canonically ordered.
    log.events = events;
    parent
}

/// Verify the integrity chain: every event carries `pi_hash`/`pi_parent`
/// and every hash recomputes over the canonical order.
///
/// # Errors
/// [`ProcessError::MissingChainAttribute`] or [`ProcessError::ChainBreak`].
pub fn verify_pi_chain(log: &OcelLog) -> Result<(), ProcessError> {
    let events = log.sorted_events();
    let mut parent = String::new();
    for (index, event) in events.iter().enumerate() {
        let Some(pi_hash) = attr_text(event, "pi_hash") else {
            return Err(ProcessError::MissingChainAttribute { index });
        };
        let Some(pi_parent) = attr_text(event, "pi_parent") else {
            return Err(ProcessError::MissingChainAttribute { index });
        };
        if pi_parent != parent {
            return Err(ProcessError::ChainBreak {
                index,
                detail: "pi_parent link mismatch",
            });
        }
        let input = format!("{parent}\n{}", canonical_event_json(event));
        let digest = hex::encode(blake3::hash(input.as_bytes()).as_bytes());
        if pi_hash != digest {
            return Err(ProcessError::ChainBreak {
                index,
                detail: "pi_hash mismatch",
            });
        }
        parent = pi_hash;
    }
    Ok(())
}

/// The canonical JSON [`Value`] for one event.
fn canonical_event_value(event: &OcelEvent) -> Value {
    let mut root = Json::new();
    root.push(
        "id",
        event
            .id
            .clone()
            .map_or(Value::Null, |id| Value::from(id.as_str())),
    );
    root.push("time", Value::from(event.time.as_str()));
    root.push("type", Value::from(event.event_type.as_str()));
    let mut relationships: Vec<Value> = Vec::new();
    let mut sorted: Vec<&EventObjectRef> = event.relationships.iter().collect();
    sorted.sort_by(|a, b| (&a.object_id, &a.qualifier).cmp(&(&b.object_id, &b.qualifier)));
    for reference in sorted {
        let mut pair = Json::new();
        pair.push("objectId", Value::from(reference.object_id.as_str()));
        pair.push("qualifier", Value::from(reference.qualifier.as_str()));
        relationships.push(Value::Object(pair));
    }
    root.push("relationships", Value::Array(relationships));
    Value::Object(root)
}

/// One attribute value as a JSON [`Value`].
pub(crate) fn attr_to_value(item: &AttrValue) -> Value {
    match item {
        AttrValue::Text(text) => Value::from(text.as_str()),
        AttrValue::Number(number) => Value::Number(number.clone()),
        AttrValue::Bool(flag) => Value::Bool(*flag),
        AttrValue::Null => Value::Null,
    }
}

/// 16 hex chars of one digest.
pub(crate) fn short_digest(digest: &blake3::Hash) -> String {
    let full = hex::encode(digest.as_bytes());
    full.chars().take(16).collect()
}
