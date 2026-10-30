// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Schema-tolerant OCEL ingestion over `purrdf_lex::json`.
//!
//! Provenance: key-alias tables and both attribute shapes follow ex4pm
//! `OCEL.normalize/1` (legacy `ocel:*` keys accepted); the dual
//! `object_ids`/`relationships` event shape, `objectId`/`object_id` aliases,
//! and the absent-qualifier-as-empty parity follow wasm4pm `models.rs`; the
//! 512 MB guard is wasm4pm `ocel_v2.rs`'s policy.

use purrdf_lex::json::{self, Object as Json, Value};
use std::collections::BTreeMap;

use crate::error::ProcessError;
use crate::log::model::{
    AttrValue, EventObjectRef, ObjectChange, ObjectRelationship, OcelEvent, OcelLog, OcelObject,
    OcelType, OcelTypeAttribute,
};

/// The ingestion byte ceiling (wasm4pm `ocel_v2.rs`'s 512 MB policy).
pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// Parse one OCEL JSON document.
///
/// Accepts the OCEL 2.0 canonical shape (`objectTypes`/`eventTypes`/
/// `objects`/`events`, qualified `relationships`) and the legacy 1.0
/// dialect (`ocel:eid`/`ocel:activity`/`ocel:timestamp`/`omap`/`ocel:vmap`).
///
/// # Errors
/// [`ProcessError::InputTooLarge`], [`ProcessError::Json`],
/// [`ProcessError::MissingEvents`]/[`ProcessError::MissingObjects`],
/// [`ProcessError::MissingField`], [`ProcessError::UnsupportedAttributeShape`],
/// or [`ProcessError::InvalidRelationship`].
pub fn from_json_str(text: &str) -> Result<OcelLog, ProcessError> {
    guard_size(text)?;
    let root = json::read(text).map_err(ProcessError::Json)?;
    parse_log_document(&root)
}

/// Parse Newline-Delimited JSON. Each non-empty line is either a full log
/// document (an object carrying `events` — merged in document order) or a
/// single event object (ex4pm `StreamOcel`'s per-line `ocel:*` dialect).
///
/// # Errors
/// As [`from_json_str`], plus [`ProcessError::Json`] for any malformed line.
pub fn from_ndjson_str(text: &str) -> Result<OcelLog, ProcessError> {
    guard_size(text)?;
    let mut log = OcelLog::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = json::read(line).map_err(ProcessError::Json)?;
        match &value {
            Value::Object(object) => {
                if object.contains_key("events") || object.contains_key("ocel:events") {
                    let mut parsed = parse_log_document(&value)?;
                    log.event_types.append(&mut parsed.event_types);
                    log.object_types.append(&mut parsed.object_types);
                    log.events.append(&mut parsed.events);
                    log.objects.append(&mut parsed.objects);
                    log.object_relationships
                        .append(&mut parsed.object_relationships);
                } else {
                    let event = parse_event(object, log.events.len())?;
                    log.events.push(event);
                }
            }
            _ => {
                return Err(ProcessError::Json(json::Error::new(
                    json::ErrorKind::Expected("a JSON object per line"),
                    0,
                )));
            }
        }
    }
    Ok(log)
}

fn guard_size(text: &str) -> Result<(), ProcessError> {
    let bytes = text.len() as u64;
    if bytes > MAX_INPUT_BYTES {
        return Err(ProcessError::InputTooLarge {
            bytes,
            limit: MAX_INPUT_BYTES,
        });
    }
    Ok(())
}

fn parse_log_document(root: &Value) -> Result<OcelLog, ProcessError> {
    let Value::Object(root) = root else {
        return Err(ProcessError::Json(json::Error::new(
            json::ErrorKind::Expected("a log document object"),
            0,
        )));
    };
    let events_json = lookup(root, &["events", "ocel:events"])
        .ok_or(ProcessError::MissingEvents)?
        .clone();
    let objects_json = lookup(root, &["objects", "ocel:objects"])
        .ok_or(ProcessError::MissingObjects)?
        .clone();
    let Value::Array(event_items) = &events_json else {
        return Err(ProcessError::InvalidRelationship {
            context: "events must be an array".to_owned(),
        });
    };
    let mut events = Vec::with_capacity(event_items.len());
    for (index, item) in event_items.iter().enumerate() {
        let Value::Object(object) = item else {
            return Err(ProcessError::InvalidRelationship {
                context: format!("event[{index}] must be an object"),
            });
        };
        events.push(parse_event(object, index)?);
    }
    let Value::Array(object_items) = &objects_json else {
        return Err(ProcessError::InvalidRelationship {
            context: "objects must be an array".to_owned(),
        });
    };
    let mut objects = Vec::with_capacity(object_items.len());
    for (index, item) in object_items.iter().enumerate() {
        let Value::Object(object) = item else {
            return Err(ProcessError::InvalidRelationship {
                context: format!("object[{index}] must be an object"),
            });
        };
        objects.push(parse_object(object, index)?);
    }
    let event_types = root
        .get("eventTypes")
        .or_else(|| root.get("event_types"))
        .map_or(Ok(Vec::new()), parse_type_list)?;
    let object_types = root
        .get("objectTypes")
        .or_else(|| root.get("object_types"))
        .map_or(Ok(Vec::new()), parse_type_list)?;
    let object_relationships = root
        .get("objectRelationships")
        .or_else(|| root.get("object_relationships"))
        .or_else(|| root.get("o2o"))
        .map_or(Ok(Vec::new()), parse_o2o_list)?;
    // Single-document logs merge object embedded relationships and top-level
    // O2O the same way: embedded stay on their object; top-level land on the
    // log.
    Ok(OcelLog {
        event_types,
        object_types,
        events,
        objects,
        object_relationships,
    })
}

fn parse_event(object: &Json, index: usize) -> Result<OcelEvent, ProcessError> {
    let document = format!("event[{index}]");
    let id = lookup(object, &["id", "ocel:eid"])
        .map_or(Ok(None), |value| as_string(value, &document).map(Some))?;
    let event_type = lookup(object, &["type", "activity", "ocel:activity", "event_type"])
        .ok_or(ProcessError::MissingField {
            document: document.clone(),
            field: "type/activity/ocel:activity",
        })
        .and_then(|value| as_string(value, &document))?;
    let time = lookup(object, &["time", "timestamp", "ocel:timestamp"])
        .ok_or(ProcessError::MissingField {
            document: document.clone(),
            field: "time/timestamp/ocel:timestamp",
        })
        .and_then(|value| as_string(value, &document))?;
    let attributes = lookup(object, &["attributes", "ocel:vmap"])
        .map_or(Ok(BTreeMap::new()), |value| attribute_map(value, &document))?;
    let object_ids = lookup(object, &["omap", "object_ids"])
        .map_or(Ok(Vec::new()), |value| string_list(value, &document))?;
    let relationships = lookup(object, &["relationships", "object_refs"])
        .map_or(Ok(Vec::new()), |value| event_refs(value, &document))?;
    Ok(OcelEvent {
        id,
        event_type,
        time,
        attributes,
        object_ids,
        relationships,
    })
}

fn parse_object(object: &Json, index: usize) -> Result<OcelObject, ProcessError> {
    let document = format!("object[{index}]");
    let id = lookup(object, &["id", "ocel:oid"])
        .ok_or(ProcessError::MissingField {
            document: document.clone(),
            field: "id/ocel:oid",
        })
        .and_then(|value| as_string(value, &document))?;
    let object_type = lookup(object, &["type", "ocel:type", "object_type"])
        .ok_or(ProcessError::MissingField {
            document: document.clone(),
            field: "type/ocel:type/object_type",
        })
        .and_then(|value| as_string(value, &document))?;
    // Both attribute shapes: a plain map, or the {name, value, time} pair
    // list (ex4pm `normalize_object_attributes/1`). Time-stamped pairs land
    // in `changes`; the current attribute map keeps the latest value per
    // name (absent time sorts first).
    let mut attributes: BTreeMap<String, AttrValue> = BTreeMap::new();
    let mut changes: Vec<ObjectChange> = Vec::new();
    if let Some(value) = lookup(object, &["attributes", "ocel:vmap"]) {
        match value {
            Value::Object(map) => {
                for (name, item) in map.iter() {
                    attributes.insert(name.clone(), attr_value(item, &document)?);
                }
            }
            Value::Array(pairs) => {
                let mut entries: Vec<(Option<String>, String, AttrValue)> = Vec::new();
                for pair in pairs {
                    let Value::Object(pair) = pair else {
                        return Err(ProcessError::UnsupportedAttributeShape {
                            context: format!("{document}.attributes"),
                        });
                    };
                    let name = lookup(pair, &["name", "attribute_name"])
                        .ok_or(ProcessError::MissingField {
                            document: document.clone(),
                            field: "attributes[].name",
                        })
                        .and_then(|value| as_string(value, &document))?;
                    let item = lookup(pair, &["value"]).ok_or(ProcessError::MissingField {
                        document: document.clone(),
                        field: "attributes[].value",
                    })?;
                    let item = attr_value(item, &document)?;
                    let time = lookup(pair, &["time"])
                        .map_or(Ok(None), |value| as_string(value, &document).map(Some))?;
                    entries.push((time, name, item));
                }
                entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
                for (time, name, item) in entries {
                    match time {
                        Some(time) => {
                            attributes.insert(name.clone(), item.clone());
                            changes.push(ObjectChange {
                                time,
                                name,
                                value: item,
                            });
                        }
                        None => {
                            attributes.insert(name, item);
                        }
                    }
                }
            }
            _ => {
                return Err(ProcessError::UnsupportedAttributeShape {
                    context: format!("{document}.attributes"),
                });
            }
        }
    }
    // Unknown scalar top-level keys fold into attributes (ex4pm folds them
    // as untime-stamped entries); unknown object/array keys are refused.
    for (name, item) in object.iter() {
        if is_known_object_key(name) {
            continue;
        }
        match item {
            Value::Object(_) | Value::Array(_) => {}
            _ => {
                attributes.insert(name.clone(), attr_value(item, &document)?);
            }
        }
    }
    let relationships = lookup(object, &["relationships"])
        .map_or(Ok(Vec::new()), |value| event_refs(value, &document))?;
    Ok(OcelObject {
        id,
        object_type,
        attributes,
        changes,
        relationships,
    })
}

fn is_known_object_key(name: &str) -> bool {
    matches!(
        name,
        "id" | "ocel:oid"
            | "type"
            | "ocel:type"
            | "object_type"
            | "attributes"
            | "ocel:vmap"
            | "relationships"
    )
}

fn parse_type_list(value: &Value) -> Result<Vec<OcelType>, ProcessError> {
    let Value::Array(items) = value else {
        return Err(ProcessError::InvalidRelationship {
            context: "type declarations must be an array".to_owned(),
        });
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            // Shorthand: a bare type name (wasm4pm parity).
            Value::String(name) => out.push(OcelType {
                name: name.clone(),
                attributes: Vec::new(),
            }),
            Value::Object(record) => {
                let name = lookup(record, &["name", "objectType", "eventType"])
                    .ok_or(ProcessError::MissingField {
                        document: "type declaration".to_owned(),
                        field: "name",
                    })
                    .and_then(|value| as_string(value, "type declaration"))?;
                let attributes = lookup(record, &["attributes"]).map_or(
                    Ok(Vec::new()),
                    |value| match value {
                        Value::Array(attrs) => {
                            let mut out = Vec::with_capacity(attrs.len());
                            for attr in attrs {
                                let Value::Object(attr) = attr else {
                                    return Err(ProcessError::UnsupportedAttributeShape {
                                        context: "type attributes".to_owned(),
                                    });
                                };
                                let attr_name = lookup(attr, &["name"])
                                    .ok_or(ProcessError::MissingField {
                                        document: "type attribute".to_owned(),
                                        field: "name",
                                    })
                                    .and_then(|value| as_string(value, "type attribute"))?;
                                let attribute_type = lookup(attr, &["attribute_type", "type"])
                                    .map_or(Ok(String::new()), |value| {
                                        as_string(value, "type attribute")
                                    })?;
                                out.push(OcelTypeAttribute {
                                    name: attr_name,
                                    attribute_type,
                                });
                            }
                            Ok(out)
                        }
                        _ => Err(ProcessError::UnsupportedAttributeShape {
                            context: "type attributes".to_owned(),
                        }),
                    },
                )?;
                out.push(OcelType { name, attributes });
            }
            _ => {
                return Err(ProcessError::UnsupportedAttributeShape {
                    context: "type declarations".to_owned(),
                });
            }
        }
    }
    Ok(out)
}

fn parse_o2o_list(value: &Value) -> Result<Vec<ObjectRelationship>, ProcessError> {
    let Value::Array(items) = value else {
        return Err(ProcessError::InvalidRelationship {
            context: "o2o must be an array".to_owned(),
        });
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(record) = item else {
            return Err(ProcessError::InvalidRelationship {
                context: "o2o entries must be objects".to_owned(),
            });
        };
        let source_id = lookup(record, &["sourceId", "source_id", "objectId"])
            .ok_or(ProcessError::MissingField {
                document: "o2o entry".to_owned(),
                field: "sourceId",
            })
            .and_then(|value| as_string(value, "o2o entry"))?;
        let target_id = lookup(record, &["targetId", "target_id"])
            .ok_or(ProcessError::MissingField {
                document: "o2o entry".to_owned(),
                field: "targetId",
            })
            .and_then(|value| as_string(value, "o2o entry"))?;
        let qualifier = lookup(record, &["qualifier"])
            .map_or(Ok(String::new()), |value| as_string(value, "o2o entry"))?;
        out.push(ObjectRelationship {
            source_id,
            target_id,
            qualifier,
        });
    }
    Ok(out)
}

/// Both attribute shapes for events: a name-to-scalar map, or a
/// `{name, value}` pair list (ex4pm `normalize_attributes/1`).
fn attribute_map(
    value: &Value,
    document: &str,
) -> Result<BTreeMap<String, AttrValue>, ProcessError> {
    let mut out = BTreeMap::new();
    match value {
        Value::Object(map) => {
            for (name, item) in map.iter() {
                out.insert(name.clone(), attr_value(item, document)?);
            }
        }
        Value::Array(pairs) => {
            for pair in pairs {
                let Value::Object(pair) = pair else {
                    return Err(ProcessError::UnsupportedAttributeShape {
                        context: format!("{document}.attributes"),
                    });
                };
                let name = lookup(pair, &["name", "attribute_name"])
                    .ok_or(ProcessError::MissingField {
                        document: document.to_owned(),
                        field: "attributes[].name",
                    })
                    .and_then(|value| as_string(value, document))?;
                let item = lookup(pair, &["value"]).ok_or(ProcessError::MissingField {
                    document: document.to_owned(),
                    field: "attributes[].value",
                })?;
                out.insert(name, attr_value(item, document)?);
            }
        }
        _ => {
            return Err(ProcessError::UnsupportedAttributeShape {
                context: format!("{document}.attributes"),
            });
        }
    }
    Ok(out)
}

fn attr_value(value: &Value, document: &str) -> Result<AttrValue, ProcessError> {
    match value {
        Value::String(text) => Ok(AttrValue::Text(text.clone())),
        Value::Number(number) => Ok(AttrValue::Number(number.clone())),
        Value::Bool(flag) => Ok(AttrValue::Bool(*flag)),
        Value::Null => Ok(AttrValue::Null),
        Value::Array(_) | Value::Object(_) => Err(ProcessError::UnsupportedAttributeShape {
            context: document.to_owned(),
        }),
    }
}

fn event_refs(value: &Value, document: &str) -> Result<Vec<EventObjectRef>, ProcessError> {
    let Value::Array(items) = value else {
        return Err(ProcessError::InvalidRelationship {
            context: format!("{document}.relationships must be an array"),
        });
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(record) = item else {
            return Err(ProcessError::InvalidRelationship {
                context: format!("{document}.relationships entries must be objects"),
            });
        };
        let object_id = lookup(record, &["objectId", "object_id", "oid"])
            .ok_or(ProcessError::MissingField {
                document: document.to_owned(),
                field: "relationships[].objectId",
            })
            .and_then(|value| as_string(value, document))?;
        let qualifier = lookup(record, &["qualifier"])
            .map_or(Ok(String::new()), |value| as_string(value, document))?;
        out.push(EventObjectRef {
            object_id,
            qualifier,
        });
    }
    Ok(out)
}

fn string_list(value: &Value, document: &str) -> Result<Vec<String>, ProcessError> {
    let Value::Array(items) = value else {
        return Err(ProcessError::InvalidRelationship {
            context: format!("{document}.omap must be an array"),
        });
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(as_string(item, document)?);
    }
    Ok(out)
}

fn as_string(value: &Value, document: &str) -> Result<String, ProcessError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        _ => Err(ProcessError::InvalidRelationship {
            context: format!("{document} expected a string"),
        }),
    }
}

fn lookup<'a>(object: &'a Json, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| object.get(name))
}
