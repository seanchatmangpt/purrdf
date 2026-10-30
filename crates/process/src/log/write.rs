// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The deterministic OCEL 2.0 JSON writer.
//!
//! Provenance: fixed key order and the relationships fallback follow ex4pm
//! `ocel_writer.ex`; JSON bytes come from `purrdf_lex::json::write`.

use purrdf_lex::json::{Format, Object as Json, Value, write};

use crate::log::integrity::attr_to_value;
use crate::log::model::{
    AttrValue, EventObjectRef, ObjectChange, OcelEvent, OcelLog, OcelObject, OcelType,
};

/// Serialize one log to OCEL 2.0 JSON in the writer's canonical order:
/// types by name, objects by id, events by `(time, id)` with canonical ids
/// minted where absent; attributes as name-sorted pairs, changes as
/// `{name, value, time}` pairs after them; an event whose qualified
/// relationships are empty projects its bare `omap` list to
/// `{"objectId", "qualifier": "involved"}` pairs (ex4pm's fallback).
#[must_use]
pub fn to_json_string(log: &OcelLog) -> String {
    write(&log_value(log), Format::COMPACT)
}

/// The log as a canonical JSON [`Value`].
#[must_use]
pub fn log_value(log: &OcelLog) -> Value {
    let mut root = Json::new();
    root.push(
        "objectTypes",
        Value::Array(
            sorted_types(&log.object_types)
                .into_iter()
                .map(type_value)
                .collect(),
        ),
    );
    root.push(
        "eventTypes",
        Value::Array(
            sorted_types(&log.event_types)
                .into_iter()
                .map(type_value)
                .collect(),
        ),
    );
    let mut objects: Vec<Value> = Vec::new();
    for object in log.sorted_objects() {
        objects.push(object_value(&object));
    }
    root.push("objects", Value::Array(objects));
    let events: Vec<Value> = log.sorted_events().iter().map(event_value).collect();
    root.push("events", Value::Array(events));
    Value::Object(root)
}

/// Types sorted by name.
fn sorted_types(types: &[OcelType]) -> Vec<&OcelType> {
    let mut sorted: Vec<&OcelType> = types.iter().collect();
    sorted.sort_by_key(|t| t.name.clone());
    sorted
}

/// One declared type as JSON.
fn type_value(t: &OcelType) -> Value {
    let mut record = Json::new();
    record.push("name", Value::from(t.name.as_str()));
    let mut attributes = Vec::new();
    for attribute in &t.attributes {
        let mut pair = Json::new();
        pair.push("name", Value::from(attribute.name.as_str()));
        pair.push(
            "attribute_type",
            Value::from(attribute.attribute_type.as_str()),
        );
        attributes.push(Value::Object(pair));
    }
    record.push("attributes", Value::Array(attributes));
    Value::Object(record)
}

/// One object as JSON.
fn object_value(object: &OcelObject) -> Value {
    let mut record = Json::new();
    record.push("id", Value::from(object.id.as_str()));
    record.push("type", Value::from(object.object_type.as_str()));
    record.push("attributes", Value::Array(object_attribute_pairs(object)));
    let relationships: Vec<Value> = object.relationships.iter().map(ref_value).collect();
    record.push("relationships", Value::Array(relationships));
    Value::Object(record)
}

/// An object's attribute pairs: the current map name-sorted, then the
/// change series as `{name, value, time}` pairs in `(time, name)` order.
fn object_attribute_pairs(object: &OcelObject) -> Vec<Value> {
    let mut out: Vec<Value> = object
        .attributes
        .iter()
        .map(|(name, item)| name_value_pair(name, item))
        .collect();
    let mut sorted_changes: Vec<&ObjectChange> = object.changes.iter().collect();
    sorted_changes.sort_by(|a, b| (&a.time, &a.name).cmp(&(&b.time, &b.name)));
    for change in sorted_changes {
        let mut pair = Json::new();
        pair.push("name", Value::from(change.name.as_str()));
        pair.push("value", attr_to_value(&change.value));
        pair.push("time", Value::from(change.time.as_str()));
        out.push(Value::Object(pair));
    }
    out
}

/// One `{name, value}` pair.
fn name_value_pair(name: &str, item: &AttrValue) -> Value {
    let mut pair = Json::new();
    pair.push("name", Value::from(name));
    pair.push("value", attr_to_value(item));
    Value::Object(pair)
}

/// One qualified object reference as JSON.
fn ref_value(reference: &EventObjectRef) -> Value {
    let mut pair = Json::new();
    pair.push("objectId", Value::from(reference.object_id.as_str()));
    pair.push("qualifier", Value::from(reference.qualifier.as_str()));
    Value::Object(pair)
}

/// One event as JSON, including the omap fallback.
fn event_value(event: &OcelEvent) -> Value {
    let mut record = Json::new();
    let id = event
        .id
        .clone()
        .unwrap_or_else(|| crate::log::integrity::canonical_event_id(event));
    record.push("id", Value::from(id.as_str()));
    record.push("type", Value::from(event.event_type.as_str()));
    record.push("time", Value::from(event.time.as_str()));
    let attributes: Vec<Value> = event
        .attributes
        .iter()
        .map(|(name, item)| name_value_pair(name, item))
        .collect();
    record.push("attributes", Value::Array(attributes));
    let relationships: Vec<Value> = if event.relationships.is_empty() {
        event
            .object_ids
            .iter()
            .map(|object_id| {
                ref_value(&EventObjectRef {
                    object_id: object_id.clone(),
                    qualifier: "involved".to_owned(),
                })
            })
            .collect()
    } else {
        event.relationships.iter().map(ref_value).collect()
    };
    record.push("relationships", Value::Array(relationships));
    Value::Object(record)
}
