//! Columnar projection of an OCEL log over the cgap substrate.
//!
//! Donor of the shape: purrdf's OCEL model (`super::model`), projected into
//! cgap-arrow `Batch` tables (ticket T1's layer). Determinism: every table
//! is built over the canonical orders the writer uses (types by name,
//! objects by id, events by `(time, id)` with canonically minted ids,
//! changes by `(time, name)`, attributes name-sorted) — the same orders
//! `super::write::to_json_string` emits, so a columnar store can reproduce
//! the JSON contract from row order. Performance posture: one pass per
//! table, partitioned value columns (no per-row unions), single-copy into
//! cgap's 64-byte-aligned buffers (the builders' contract), `u32` row
//! indices linking tables instead of id strings.


use cgap_arrow::{Batch, BatchBuilder, ColumnBuilder};

use crate::error::ProcessError;
use crate::log::integrity::canonical_event_id;
use crate::log::model::{AttrValue, OcelLog};

/// The seven projected tables.
pub struct LogTables {
    /// `events`: id, type, time (canonical `(time, id)` order).
    pub events: Batch,
    /// `event_attrs` EAV: event row index, name, kind tag, partitioned
    /// value columns (text and exact numeric lexeme as Utf8, bool).
    pub event_attrs: Batch,
    /// `event_object_refs`: event row index, object id, qualifier (the
    /// `omap` fallback emits `qualifier = "involved"` rows).
    pub event_object_refs: Batch,
    /// `objects`: id, type (canonical id order).
    pub objects: Batch,
    /// `object_changes`: object row index, time, name, kind tag,
    /// partitioned values.
    pub object_changes: Batch,
    /// `o2o`: source id, target id, qualifier.
    pub o2o: Batch,
    /// `types`: table family (`object`/`event`), type name, attribute
    /// name, declared attribute type.
    pub types: Batch,
}

impl core::fmt::Debug for LogTables {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Structural summary: per-table row counts, never column contents.
        let counts: Vec<(&'static str, usize)> = self
            .iter()
            .map(|(name, table)| (name, table.row_count()))
            .collect();
        f.debug_struct("LogTables").field("tables", &counts).finish()
    }
}

impl LogTables {
    /// The tables with their names, in the canonical projection order.
    #[must_use]
    pub fn iter(&self) -> LogTablesIter<'_> {
        LogTablesIter {
            tables: [
                ("events", &self.events),
                ("event_attrs", &self.event_attrs),
                ("event_object_refs", &self.event_object_refs),
                ("objects", &self.objects),
                ("object_changes", &self.object_changes),
                ("o2o", &self.o2o),
                ("types", &self.types),
            ],
            index: 0,
        }
    }
}

/// Iterator over the projected tables with their names.
pub struct LogTablesIter<'a> {
    tables: [(&'static str, &'a Batch); 7],
    index: usize,
}

impl core::fmt::Debug for LogTablesIter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LogTablesIter")
            .field("index", &self.index)
            .finish()
    }
}

impl<'a> Iterator for LogTablesIter<'a> {
    type Item = (&'static str, &'a Batch);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.tables.get(self.index).copied();
        if item.is_some() {
            self.index += 1;
        }
        item
    }
}

/// One attribute value's kind tag (EAV partitioning).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// Text payload.
    Text = 1,
    /// Exact numeric payload (the lexeme is the value).
    Number = 2,
    /// Boolean payload.
    Bool = 3,
    /// No payload (JSON null).
    Null = 4,
}

impl ValueKind {
    /// The `u8` tag stored in the kind column.
    #[must_use]
    pub fn tag(self) -> u8 {
        self as u8
    }
}

/// The partitioned value columns of one EAV table: rectangular, with each
/// row's payload in exactly one partition (empty string / zero elsewhere),
/// occupancy tagged by `kind`.
struct ValueColumns {
    kind: cgap_arrow::PrimitiveBuilder<u8>,
    text: cgap_arrow::VarlenBuilder<u32>,
    number: cgap_arrow::VarlenBuilder<u32>,
    flag: cgap_arrow::PrimitiveBuilder<i8>,
}

impl ValueColumns {
    fn new() -> Self {
        Self {
            kind: cgap_arrow::PrimitiveBuilder::new(),
            text: cgap_arrow::VarlenBuilder::new(),
            number: cgap_arrow::VarlenBuilder::new(),
            flag: cgap_arrow::PrimitiveBuilder::new(),
        }
    }

    /// Partition one value into the columns (one row).
    fn push(&mut self, value: &AttrValue) {
        match value {
            AttrValue::Text(t) => {
                self.kind.push(Some(ValueKind::Text.tag()));
                self.text.push(Some(t.as_bytes()));
                self.number.push(Some(&[]));
                self.flag.push(Some(0));
            }
            AttrValue::Number(n) => {
                self.kind.push(Some(ValueKind::Number.tag()));
                self.text.push(Some(&[]));
                self.number.push(Some(n.lexeme().as_bytes()));
                self.flag.push(Some(0));
            }
            AttrValue::Bool(b) => {
                self.kind.push(Some(ValueKind::Bool.tag()));
                self.text.push(Some(&[]));
                self.number.push(Some(&[]));
                self.flag.push(Some(i8::from(*b)));
            }
            AttrValue::Null => {
                self.kind.push(Some(ValueKind::Null.tag()));
                self.text.push(Some(&[]));
                self.number.push(Some(&[]));
                self.flag.push(Some(0));
            }
        }
    }

    /// Freeze into named columns for a batch.
    fn into_named(self) -> Vec<(&'static str, ColumnBuilder)> {
        vec![
            ("kind", ColumnBuilder::UInt8(self.kind)),
            ("value_text", ColumnBuilder::Utf8(self.text)),
            ("value_number", ColumnBuilder::Utf8(self.number)),
            ("value_bool", ColumnBuilder::Int8(self.flag)),
        ]
    }
}

/// Attach a finished [`Batch`]-named set of columns under `name`, refusing
/// via [`ProcessError::BatchRefused`].
fn batch_of(
    builder: BatchBuilder,
    table: &'static str,
) -> Result<Batch, ProcessError> {
    builder.finish().map_err(|error| ProcessError::BatchRefused {
        table,
        detail: format!("{error}"),
    })
}

/// Project one log into the seven canonical tables.
///
/// # Errors
/// [`ProcessError::BatchRefused`] when the batch layer refuses a table
/// (a projection bug: lengths are built in lockstep).
pub fn to_batches(log: &OcelLog) -> Result<LogTables, ProcessError> {
    let events = log.sorted_events();
    let objects = log.sorted_objects();
    let mut event_id = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut event_type = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut event_time = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut attrs = ValueColumns::new();
    let mut attr_event_index = cgap_arrow::PrimitiveBuilder::<u32>::new();
    let mut attr_name = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut ref_event_index = cgap_arrow::PrimitiveBuilder::<u32>::new();
    let mut ref_object_id = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut ref_qualifier = cgap_arrow::VarlenBuilder::<u32>::new();
    for (index, event) in events.iter().enumerate() {
        let row = u32::try_from(index).unwrap_or(u32::MAX);
        let id = event
            .id
            .clone()
            .unwrap_or_else(|| canonical_event_id(event));
        event_id.push(Some(id.as_bytes()));
        event_type.push(Some(event.event_type.as_bytes()));
        event_time.push(Some(event.time.as_bytes()));
        // omap fallback mirrors the writer's rule: bare object ids become
        // "involved" qualifiers when no qualified relationships exist.
        if event.relationships.is_empty() {
            for object_id in &event.object_ids {
                ref_event_index.push(Some(row));
                ref_object_id.push(Some(object_id.as_bytes()));
                ref_qualifier.push(Some(b"involved"));
            }
        } else {
            for reference in &event.relationships {
                ref_event_index.push(Some(row));
                ref_object_id.push(Some(reference.object_id.as_bytes()));
                ref_qualifier.push(Some(reference.qualifier.as_bytes()));
            }
        }
        // Attributes iterate name-sorted (BTreeMap order = the writer's).
        for (name, value) in &event.attributes {
            attr_event_index.push(Some(row));
            attr_name.push(Some(name.as_bytes()));
            attrs.push(value);
        }
    }
    let events_batch = batch_of(
        BatchBuilder::new()
            .with_column("id", ColumnBuilder::Utf8(event_id))
            .with_column("type", ColumnBuilder::Utf8(event_type))
            .with_column("time", ColumnBuilder::Utf8(event_time)),
        "events",
    )?;
    let event_attrs_batch = {
        let mut builder = BatchBuilder::new().with_column(
            "event_index",
            ColumnBuilder::UInt32(attr_event_index),
        );
        builder = builder.with_column("name", ColumnBuilder::Utf8(attr_name));
        for (name, column) in attrs.into_named() {
            builder = builder.with_column(name, column);
        }
        batch_of(builder, "event_attrs")?
    };
    let refs_batch = batch_of(
        BatchBuilder::new()
            .with_column("event_index", ColumnBuilder::UInt32(ref_event_index))
            .with_column("object_id", ColumnBuilder::Utf8(ref_object_id))
            .with_column("qualifier", ColumnBuilder::Utf8(ref_qualifier)),
        "event_object_refs",
    )?;
    let objects_batch = project_objects(&objects)?;
    let object_changes_batch = project_object_changes(&objects)?;
    let o2o_batch = {
        let mut source = cgap_arrow::VarlenBuilder::<u32>::new();
        let mut target = cgap_arrow::VarlenBuilder::<u32>::new();
        let mut qualifier = cgap_arrow::VarlenBuilder::<u32>::new();
        for relationship in &log.object_relationships {
            source.push(Some(relationship.source_id.as_bytes()));
            target.push(Some(relationship.target_id.as_bytes()));
            qualifier.push(Some(relationship.qualifier.as_bytes()));
        }
        batch_of(
            BatchBuilder::new()
                .with_column("source_id", ColumnBuilder::Utf8(source))
                .with_column("target_id", ColumnBuilder::Utf8(target))
                .with_column("qualifier", ColumnBuilder::Utf8(qualifier)),
            "o2o",
        )?
    };
    let types_batch = project_types(log)?;
    Ok(LogTables {
        events: events_batch,
        event_attrs: event_attrs_batch,
        event_object_refs: refs_batch,
        objects: objects_batch,
        object_changes: object_changes_batch,
        o2o: o2o_batch,
        types: types_batch,
    })
}

/// `objects`: id, type — canonical id order.
fn project_objects(objects: &[crate::log::model::OcelObject]) -> Result<Batch, ProcessError> {
    let mut id = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut object_type = cgap_arrow::VarlenBuilder::<u32>::new();
    for object in objects {
        id.push(Some(object.id.as_bytes()));
        object_type.push(Some(object.object_type.as_bytes()));
    }
    batch_of(
        BatchBuilder::new()
            .with_column("id", ColumnBuilder::Utf8(id))
            .with_column("type", ColumnBuilder::Utf8(object_type)),
        "objects",
    )
}

/// `object_changes`: object row index, time, name, partitioned values —
/// canonical `(time, name)` order per object, objects in id order.
fn project_object_changes(
    objects: &[crate::log::model::OcelObject],
) -> Result<Batch, ProcessError> {
    let mut object_index = cgap_arrow::PrimitiveBuilder::<u32>::new();
    let mut time = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut name = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut values = ValueColumns::new();
    for (index, object) in objects.iter().enumerate() {
        let row = u32::try_from(index).unwrap_or(u32::MAX);
        let mut sorted: Vec<&crate::log::model::ObjectChange> = object.changes.iter().collect();
        sorted.sort_by(|a, b| (&a.time, &a.name).cmp(&(&b.time, &b.name)));
        for change in sorted {
            object_index.push(Some(row));
            time.push(Some(change.time.as_bytes()));
            name.push(Some(change.name.as_bytes()));
            values.push(&change.value);
        }
    }
    let mut builder = BatchBuilder::new()
        .with_column("object_index", ColumnBuilder::UInt32(object_index))
        .with_column("time", ColumnBuilder::Utf8(time))
        .with_column("name", ColumnBuilder::Utf8(name));
    for (column_name, column) in values.into_named() {
        builder = builder.with_column(column_name, column);
    }
    batch_of(builder, "object_changes")
}

/// `types`: family, type name, attribute name, declared attribute type —
/// name-sorted per family.
fn project_types(log: &OcelLog) -> Result<Batch, ProcessError> {
    let mut family = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut type_name = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut attr_name = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut attr_type = cgap_arrow::VarlenBuilder::<u32>::new();
    let mut sorted_object: Vec<&crate::log::model::OcelType> = log.object_types.iter().collect();
    sorted_object.sort_by(|a, b| a.name.cmp(&b.name));
    for object_type in sorted_object {
        for attribute in &object_type.attributes {
            family.push(Some(b"object"));
            type_name.push(Some(object_type.name.as_bytes()));
            attr_name.push(Some(attribute.name.as_bytes()));
            attr_type.push(Some(attribute.attribute_type.as_bytes()));
        }
    }
    let mut sorted_event: Vec<&crate::log::model::OcelType> = log.event_types.iter().collect();
    sorted_event.sort_by(|a, b| a.name.cmp(&b.name));
    for event_type in sorted_event {
        for attribute in &event_type.attributes {
            family.push(Some(b"event"));
            type_name.push(Some(event_type.name.as_bytes()));
            attr_name.push(Some(attribute.name.as_bytes()));
            attr_type.push(Some(attribute.attribute_type.as_bytes()));
        }
    }
    batch_of(
        BatchBuilder::new()
            .with_column("family", ColumnBuilder::Utf8(family))
            .with_column("type_name", ColumnBuilder::Utf8(type_name))
            .with_column("attr_name", ColumnBuilder::Utf8(attr_name))
            .with_column("attr_type", ColumnBuilder::Utf8(attr_type)),
        "types",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::from_json_str;
    use cgap_arrow::Column;

    /// The parser courts' fixture (numeric attribute exercises the Number
    /// partition).
    const FIXTURE: &str = r#"{
      "objectTypes": [
        {"name": "order", "attributes": [{"name": "total", "attribute_type": "string"}]},
        {"name": "payment", "attributes": []}
      ],
      "eventTypes": [
        {"name": "create order", "attributes": []},
        {"name": "ship order", "attributes": []}
      ],
      "objects": [
        {
          "id": "o1",
          "type": "order",
          "attributes": [
            {"name": "total", "value": "42", "time": "2026-01-01T00:00:00Z"},
            {"name": "total", "value": "45", "time": "2026-01-02T00:00:00Z"}
          ]
        },
        {"id": "o2", "type": "payment"}
      ],
      "events": [
        {
          "id": "e1",
          "type": "create order",
          "time": "2026-01-01T00:00:00Z",
          "omap": ["o1"],
          "attributes": {"amount": 42}
        },
        {
          "id": "e2",
          "type": "ship order",
          "time": "2026-01-02T00:00:00Z",
          "relationships": [{"objectId": "o1", "qualifier": "ships"}]
        }
      ],
      "o2o": [{"sourceId": "o1", "targetId": "o2"}]
    }"#;

    /// The fixture, projected: every table is rectangular, counts match the
    /// source, and the canonical orders hold.
    #[test]
    fn fixture_projection_is_rectangular_and_ordered() {
        let log = from_json_str(FIXTURE).unwrap();
        let tables = to_batches(&log).unwrap();
        assert_eq!(tables.events.row_count(), 2);
        assert_eq!(tables.objects.row_count(), 2);
        assert_eq!(tables.o2o.row_count(), 1);
        // One declared attribute across all types (order.total).
        assert_eq!(tables.types.row_count(), 1);
        // Events carry the canonical ids and (time, id) order.
        match tables.events.get("id") {
            Some(Column::Utf8(ids)) => {
                assert_eq!(ids.row(0), b"e1");
                assert_eq!(ids.row(1), b"e2");
            }
            other => panic!("expected utf8 ids, got {other:?}"),
        }
        // e1's omap fallback: one "involved" ref row; e2's qualified ref is
        // a second row.
        assert_eq!(tables.event_object_refs.row_count(), 2);
        match tables.event_object_refs.get("qualifier") {
            Some(Column::Utf8(q)) => {
                assert_eq!(q.row(0), b"involved");
                assert_eq!(q.row(1), b"ships");
            }
            other => panic!("expected utf8 qualifiers, got {other:?}"),
        }
        // e1's single attribute: kind Number, exact lexeme "42".
        assert_eq!(tables.event_attrs.row_count(), 1);
        match tables.event_attrs.get("kind") {
            Some(Column::UInt8(k)) => assert_eq!(k.values(), &[ValueKind::Number.tag()]),
            other => panic!("expected u8 kinds, got {other:?}"),
        }
        match tables.event_attrs.get("value_number") {
            Some(Column::Utf8(n)) => assert_eq!(n.row(0), b"42"),
            other => panic!("expected utf8 number lexeme, got {other:?}"),
        }
        // o1's two stamped totals land as two change rows in (time, name).
        assert_eq!(tables.object_changes.row_count(), 2);
        match tables.object_changes.get("time") {
            Some(Column::Utf8(t)) => {
                assert_eq!(t.row(0), b"2026-01-01T00:00:00Z");
                assert_eq!(t.row(1), b"2026-01-02T00:00:00Z");
            }
            other => panic!("expected utf8 times, got {other:?}"),
        }
    }

    /// Determinism: two projections of one log build equal column content
    /// (spot-checked via row walks — Batch has no PartialEq by design).
    #[test]
    fn projection_is_deterministic() {
        let log = from_json_str(FIXTURE).unwrap();
        let first = to_batches(&log).unwrap();
        let second = to_batches(&log).unwrap();
        assert_eq!(
            first.event_attrs.row_count(),
            second.event_attrs.row_count()
        );
        match (first.events.get("time"), second.events.get("time")) {
            (Some(Column::Utf8(a)), Some(Column::Utf8(b))) => {
                assert_eq!(a.len(), b.len());
                for row in 0..a.len() {
                    assert_eq!(a.row(row), b.row(row));
                }
            }
            other => panic!("expected utf8 time columns, got {other:?}"),
        }
    }
}
