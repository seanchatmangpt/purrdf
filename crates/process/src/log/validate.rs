// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Log validation: referential integrity, timestamp shape, identity
//! uniqueness, and declared-type coverage.
//!
//! Provenance: rules follow ex4pm `validate_references/2` +
//! `validate_envelope/1` and beam4pm `Ocel.validate_envelope/2`; timestamp
//! shape is xsd:dateTime via `purrdf-xsd`.

use std::collections::BTreeSet;

use purrdf_xsd::temporal;

use crate::error::ProcessError;
use crate::log::model::OcelLog;

/// Validate one log: every referenced object id resolves, every timestamp is
/// a lexical xsd:dateTime, ids are unique, and declared types cover the
/// documents that cite them.
///
/// # Errors
/// [`ProcessError::UnknownObjectReference`],
/// [`ProcessError::InvalidTimestamp`], [`ProcessError::DuplicateIdentity`],
/// [`ProcessError::UndeclaredObjectType`], or
/// [`ProcessError::UndeclaredEventType`].
pub fn validate(log: &OcelLog) -> Result<(), ProcessError> {
    let object_ids: BTreeSet<&str> = log.objects.iter().map(|o| o.id.as_str()).collect();
    let object_types: BTreeSet<&str> = log.object_types.iter().map(|t| t.name.as_str()).collect();
    let event_types: BTreeSet<&str> = log.event_types.iter().map(|t| t.name.as_str()).collect();

    let mut seen_objects: BTreeSet<&str> = BTreeSet::new();
    for object in &log.objects {
        if !seen_objects.insert(object.id.as_str()) {
            return Err(ProcessError::DuplicateIdentity {
                kind: "object",
                id: object.id.clone(),
            });
        }
    }

    let events = log.sorted_events();
    let mut seen_events: BTreeSet<String> = BTreeSet::new();
    for event in &events {
        let id = event
            .id
            .clone()
            .unwrap_or_else(|| crate::log::integrity::canonical_event_id(event));
        if !seen_events.insert(id.clone()) {
            return Err(ProcessError::DuplicateIdentity { kind: "event", id });
        }
        for object_id in &event.object_ids {
            if !object_ids.contains(object_id.as_str()) {
                return Err(ProcessError::UnknownObjectReference {
                    subject: format!("event:{id}"),
                    object: object_id.clone(),
                });
            }
        }
        for reference in &event.relationships {
            if !object_ids.contains(reference.object_id.as_str()) {
                return Err(ProcessError::UnknownObjectReference {
                    subject: format!("event:{id}"),
                    object: reference.object_id.clone(),
                });
            }
        }
        check_time(&event.time)?;
        if !event_types.is_empty() && !event_types.contains(event.event_type.as_str()) {
            return Err(ProcessError::UndeclaredEventType {
                type_name: event.event_type.clone(),
            });
        }
    }

    for object in &log.objects {
        for change in &object.changes {
            check_time(&change.time)?;
        }
        if !object_types.is_empty() && !object_types.contains(object.object_type.as_str()) {
            return Err(ProcessError::UndeclaredObjectType {
                type_name: object.object_type.clone(),
            });
        }
        for reference in &object.relationships {
            if !object_ids.contains(reference.object_id.as_str()) {
                return Err(ProcessError::UnknownObjectReference {
                    subject: format!("object:{}", object.id),
                    object: reference.object_id.clone(),
                });
            }
        }
    }

    for relationship in &log.object_relationships {
        for object_id in [&relationship.source_id, &relationship.target_id] {
            if !object_ids.contains(object_id.as_str()) {
                return Err(ProcessError::UnknownObjectReference {
                    subject: "o2o".to_owned(),
                    object: object_id.clone(),
                });
            }
        }
    }
    Ok(())
}

/// xsd:dateTime shape check.
fn check_time(lexical: &str) -> Result<(), ProcessError> {
    if temporal::parse_datetime(lexical).is_err() {
        return Err(ProcessError::InvalidTimestamp {
            lexical: lexical.to_owned(),
        });
    }
    Ok(())
}
