// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The OCEL 2.0 in-memory model.
//!
//! Shapes follow wasm4pm `models.rs:762-880` (dual-dialect event/object
//! structs) and ex4pm `ocel.ex` (object attribute time series with
//! point-in-time resolution). Numbers keep their exact JSON lexemes
//! ([`AttrValue::Number`] wraps [`purrdf_lex::json::Number`]), so no float
//! rounding ever enters a log, a digest, or a comparison.

use purrdf_lex::json::Number;
use std::collections::BTreeMap;

/// One attribute value. OCEL scalars only: arrays and objects are extensions
/// and refused at ingestion ([`ProcessError::UnsupportedAttributeShape`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue {
    /// A string value.
    Text(String),
    /// An exact number, kept as its JSON lexeme.
    Number(Number),
    /// A boolean value.
    Bool(bool),
    /// JSON null.
    Null,
}

/// A point-in-time attribute entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueAtTime {
    /// The value at (or from) this point.
    pub value: AttrValue,
    /// The lexical xsd:dateTime of the change, if stamped.
    pub time: Option<String>,
}

/// One object attribute: a name over its value-time series (ex4pm
/// `ObjectRef.attributes`), entries sorted by time with `None` first.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectAttribute {
    /// Attribute name.
    pub name: String,
    /// The value-time series.
    pub entries: Vec<ValueAtTime>,
}

impl ObjectAttribute {
    /// The value at `as_of` (lexical xsd:dateTime comparison, ex4pm
    /// `attribute_at/3`): the last entry whose time is absent or `<= as_of`.
    /// Absent `as_of` yields the latest entry.
    #[must_use]
    pub fn value_at(&self, as_of: Option<&str>) -> Option<&AttrValue> {
        let eligible = self
            .entries
            .iter()
            .filter(|entry| match (&entry.time, as_of) {
                (None, _) => true,
                (Some(t), Some(cutoff)) => t.as_str() <= cutoff,
                (Some(_), None) => true,
            })
            .last();
        eligible.map(|entry| &entry.value)
    }
}

/// One event-to-object reference: OCEL 2.0's qualified E2O edge.
#[derive(Clone, Debug, PartialEq)]
pub struct EventObjectRef {
    /// The referenced object id.
    pub object_id: String,
    /// The E2O qualifier (wasm4pm parity: absent parses as empty).
    pub qualifier: String,
}

/// One event.
#[derive(Clone, Debug, PartialEq)]
pub struct OcelEvent {
    /// The event id; `None` is the OCEL 2.0 "unidentified" case and is
    /// minted canonically by [`crate::log::integrity`].
    pub id: Option<String>,
    /// The event type (activity).
    pub event_type: String,
    /// The lexical xsd:dateTime.
    pub time: String,
    /// Scalar attributes, name-sorted for output.
    pub attributes: BTreeMap<String, AttrValue>,
    /// OCEL 1.0 object id list (`omap`).
    pub object_ids: Vec<String>,
    /// OCEL 2.0 qualified object references.
    pub relationships: Vec<EventObjectRef>,
}

/// One object.
#[derive(Clone, Debug, PartialEq)]
pub struct OcelObject {
    /// The object id.
    pub id: String,
    /// The object type.
    pub object_type: String,
    /// Current scalar attributes (name-sorted for output).
    pub attributes: BTreeMap<String, AttrValue>,
    /// The attribute change series.
    pub changes: Vec<ObjectChange>,
    /// Embedded O2O references.
    pub relationships: Vec<EventObjectRef>,
}

/// One object attribute change.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectChange {
    /// The lexical xsd:dateTime of the change.
    pub time: String,
    /// Attribute name.
    pub name: String,
    /// The new value.
    pub value: AttrValue,
}

/// One declared type attribute (OCEL 2.0 `objectTypes`/`eventTypes`).
#[derive(Clone, Debug, PartialEq)]
pub struct OcelTypeAttribute {
    /// Attribute name.
    pub name: String,
    /// Declared attribute type spelling (e.g. `"string"`).
    pub attribute_type: String,
}

/// One declared type.
#[derive(Clone, Debug, PartialEq)]
pub struct OcelType {
    /// Type name.
    pub name: String,
    /// Declared attributes.
    pub attributes: Vec<OcelTypeAttribute>,
}

/// One object-to-object relationship (ex4pm `ObjectRelationship`).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectRelationship {
    /// The source object id.
    pub source_id: String,
    /// The target object id.
    pub target_id: String,
    /// The O2O qualifier.
    pub qualifier: String,
}

/// An object-centric event log.
///
/// `events` and `objects` stay in document order; canonical order is
/// available via [`OcelLog::sorted_events`]/[`OcelLog::sorted_objects`] and
/// is what the writer and the integrity chain use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OcelLog {
    /// Declared event types.
    pub event_types: Vec<OcelType>,
    /// Declared object types.
    pub object_types: Vec<OcelType>,
    /// Events, document order.
    pub events: Vec<OcelEvent>,
    /// Objects, document order.
    pub objects: Vec<OcelObject>,
    /// Top-level O2O relationships, document order.
    pub object_relationships: Vec<ObjectRelationship>,
}

impl OcelLog {
    /// Events in canonical `(time, id)` order (ex4pm's sort rule), ids
    /// minted canonically where absent.
    #[must_use]
    pub fn sorted_events(&self) -> Vec<OcelEvent> {
        let mut events = self.events.clone();
        for event in &mut events {
            if event.id.is_none() {
                event.id = Some(crate::log::integrity::canonical_event_id(event));
            }
        }
        events.sort_by(|a, b| (&a.time, &a.id).cmp(&(&b.time, &b.id)));
        events
    }

    /// Objects sorted by id.
    #[must_use]
    pub fn sorted_objects(&self) -> Vec<OcelObject> {
        let mut objects = self.objects.clone();
        objects.sort_by(|a, b| a.id.cmp(&b.id));
        objects
    }

    /// The declared type attribute names for one object type.
    #[must_use]
    pub fn object_type_attributes(&self, type_name: &str) -> Option<Vec<String>> {
        self.object_types
            .iter()
            .find(|t| t.name == type_name)
            .map(|t| t.attributes.iter().map(|a| a.name.clone()).collect())
    }
}
