//! Typed per-document metadata columns (ADR 0009).
//!
//! An archive can carry a metadata table: a schema declared once (field names + types)
//! and one row per document, stored **columnar** in a single rkyv blob in the footer
//! region (located via the trailer's `meta_offset`/`meta_len`, length 0 = no metadata).
//! Row `i` belongs to doc-table position `i` (arrival order), so lookups are O(1) by
//! document position and whole columns can be handed to Arrow without touching blobs.
//!
//! Types are the four scalar kinds a metadata sidecar realistically needs — `Str`,
//! `Int` (i64), `Float` (f64), `Bool` — each nullable via a one-byte-per-row validity
//! vector (metadata is ~0.1% of an archive; bit-packing would save nothing that
//! matters). String columns store all values concatenated with `u32` end offsets,
//! capping one column's text at 4 GiB — validated at build time.

use rkyv::{Archive as RkyvArchive, Deserialize, Serialize};

use crate::error::ArchiveErr;

/// The scalar type of one metadata field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaType {
    Str,
    Int,
    Float,
    Bool,
}

impl MetaType {
    pub(crate) fn code(self) -> u8 {
        match self {
            MetaType::Str => 0,
            MetaType::Int => 1,
            MetaType::Float => 2,
            MetaType::Bool => 3,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<MetaType> {
        Some(match code {
            0 => MetaType::Str,
            1 => MetaType::Int,
            2 => MetaType::Float,
            3 => MetaType::Bool,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            MetaType::Str => "str",
            MetaType::Int => "int",
            MetaType::Float => "float",
            MetaType::Bool => "bool",
        }
    }
}

/// One metadata value. `Null` is represented by `Option<MetaValue>` at the API edge.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl MetaValue {
    fn type_of(&self) -> MetaType {
        match self {
            MetaValue::Str(_) => MetaType::Str,
            MetaValue::Int(_) => MetaType::Int,
            MetaValue::Float(_) => MetaType::Float,
            MetaValue::Bool(_) => MetaType::Bool,
        }
    }
}

/// A borrowed metadata value read back out of an archived column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MetaRef<'a> {
    Str(&'a str),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// The declared schema: field names + types, in declaration order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetaSchema {
    pub fields: Vec<(String, MetaType)>,
}

impl MetaSchema {
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }

    /// Check a row's arity and value types against this schema without storing anything —
    /// the pre-flight for streaming writers that must validate *before* committing the
    /// document the row belongs to.
    pub fn validate_row(&self, row: &[Option<MetaValue>]) -> Result<(), ArchiveErr> {
        if row.len() != self.fields.len() {
            return Err(ArchiveErr::Validate(format!(
                "metadata row has {} values, schema has {} fields",
                row.len(),
                self.fields.len()
            )));
        }
        for (value, (name, declared)) in row.iter().zip(&self.fields) {
            if let Some(v) = value
                && v.type_of() != *declared
            {
                return Err(ArchiveErr::Validate(format!(
                    "metadata field '{name}' is {}, got {}",
                    declared.name(),
                    v.type_of().name()
                )));
            }
        }
        Ok(())
    }
}

/// One serialized column. Validity is one byte per row (0 = null); `Str` stores the
/// concatenated bytes plus cumulative end offsets (`ends[i]` = end of row `i`'s slice,
/// slice start = `ends[i-1]` or 0).
#[derive(RkyvArchive, Serialize, Deserialize, Debug)]
pub enum MetaColumn {
    Str {
        ends: Vec<u32>,
        bytes: Vec<u8>,
        valid: Vec<u8>,
    },
    Int {
        values: Vec<i64>,
        valid: Vec<u8>,
    },
    Float {
        values: Vec<f64>,
        valid: Vec<u8>,
    },
    Bool {
        values: Vec<u8>,
        valid: Vec<u8>,
    },
}

/// The complete metadata table as serialized into the footer blob: parallel
/// `names`/`types` (schema, `types[i]` is a [`MetaType::code`]) plus one column per field.
#[derive(RkyvArchive, Serialize, Deserialize, Debug, Default)]
pub struct MetaTable {
    pub names: Vec<String>,
    pub types: Vec<u8>,
    pub columns: Vec<MetaColumn>,
}

impl MetaTable {
    pub fn schema(&self) -> MetaSchema {
        MetaSchema {
            fields: self
                .names
                .iter()
                .zip(&self.types)
                .map(|(n, &t)| {
                    let ty = MetaType::from_code(t).expect("validated on build/read");
                    (n.clone(), ty)
                })
                .collect(),
        }
    }

    pub fn row_count(&self) -> usize {
        match self.columns.first() {
            Some(MetaColumn::Str { valid, .. })
            | Some(MetaColumn::Int { valid, .. })
            | Some(MetaColumn::Float { valid, .. })
            | Some(MetaColumn::Bool { valid, .. }) => valid.len(),
            None => 0,
        }
    }
}

impl ArchivedMetaTable {
    /// The declared schema (validated on open, so the type codes are known-good).
    pub fn schema(&self) -> MetaSchema {
        MetaSchema {
            fields: self
                .names
                .iter()
                .zip(self.types.iter())
                .map(|(n, t)| {
                    let ty = MetaType::from_code(*t).expect("validated on open");
                    (n.as_str().to_string(), ty)
                })
                .collect(),
        }
    }
}

/// Accumulates rows against a schema and produces the columnar [`MetaTable`].
#[derive(Debug)]
pub struct MetaTableBuilder {
    schema: MetaSchema,
    columns: Vec<ColumnBuilder>,
}

#[derive(Debug)]
enum ColumnBuilder {
    Str {
        ends: Vec<u32>,
        bytes: Vec<u8>,
        valid: Vec<u8>,
    },
    Int {
        values: Vec<i64>,
        valid: Vec<u8>,
    },
    Float {
        values: Vec<f64>,
        valid: Vec<u8>,
    },
    Bool {
        values: Vec<u8>,
        valid: Vec<u8>,
    },
}

impl MetaTableBuilder {
    pub fn new(schema: MetaSchema) -> Self {
        let columns = schema
            .fields
            .iter()
            .map(|(_, ty)| match ty {
                MetaType::Str => ColumnBuilder::Str {
                    ends: Vec::new(),
                    bytes: Vec::new(),
                    valid: Vec::new(),
                },
                MetaType::Int => ColumnBuilder::Int {
                    values: Vec::new(),
                    valid: Vec::new(),
                },
                MetaType::Float => ColumnBuilder::Float {
                    values: Vec::new(),
                    valid: Vec::new(),
                },
                MetaType::Bool => ColumnBuilder::Bool {
                    values: Vec::new(),
                    valid: Vec::new(),
                },
            })
            .collect();
        MetaTableBuilder { schema, columns }
    }

    pub fn schema(&self) -> &MetaSchema {
        &self.schema
    }

    /// Rehydrate a builder from a finished table so more rows can be appended — the
    /// in-place-append continuation (ADR 0010). The column shapes are identical, so this
    /// is a move, not a copy.
    pub fn from_table(table: MetaTable) -> Self {
        let schema = table.schema();
        let columns = table
            .columns
            .into_iter()
            .map(|c| match c {
                MetaColumn::Str { ends, bytes, valid } => ColumnBuilder::Str { ends, bytes, valid },
                MetaColumn::Int { values, valid } => ColumnBuilder::Int { values, valid },
                MetaColumn::Float { values, valid } => ColumnBuilder::Float { values, valid },
                MetaColumn::Bool { values, valid } => ColumnBuilder::Bool { values, valid },
            })
            .collect();
        MetaTableBuilder { schema, columns }
    }

    /// Append one row; `row[i]` corresponds to `schema.fields[i]`, `None` = null.
    /// Values must match the declared type exactly (no coercion here — the caller's
    /// API edge decides what to coerce).
    pub fn push_row(&mut self, row: Vec<Option<MetaValue>>) -> Result<(), ArchiveErr> {
        if row.len() != self.schema.fields.len() {
            return Err(ArchiveErr::Validate(format!(
                "metadata row has {} values, schema has {} fields",
                row.len(),
                self.schema.fields.len()
            )));
        }
        for (i, value) in row.into_iter().enumerate() {
            if let Some(v) = &value {
                let declared = self.schema.fields[i].1;
                if v.type_of() != declared {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata field '{}' is {}, got {}",
                        self.schema.fields[i].0,
                        declared.name(),
                        v.type_of().name()
                    )));
                }
            }
            match (&mut self.columns[i], value) {
                (ColumnBuilder::Str { ends, bytes, valid }, v) => {
                    if let Some(MetaValue::Str(s)) = v {
                        let end = bytes
                            .len()
                            .checked_add(s.len())
                            .and_then(|e| u32::try_from(e).ok());
                        let Some(end) = end else {
                            return Err(ArchiveErr::Validate(format!(
                                "metadata column '{}' exceeds 4 GiB of string data",
                                self.schema.fields[i].0
                            )));
                        };
                        bytes.extend_from_slice(s.as_bytes());
                        ends.push(end);
                        valid.push(1);
                    } else {
                        ends.push(bytes.len() as u32);
                        valid.push(0);
                    }
                }
                (ColumnBuilder::Int { values, valid }, v) => {
                    if let Some(MetaValue::Int(x)) = v {
                        values.push(x);
                        valid.push(1);
                    } else {
                        values.push(0);
                        valid.push(0);
                    }
                }
                (ColumnBuilder::Float { values, valid }, v) => {
                    if let Some(MetaValue::Float(x)) = v {
                        values.push(x);
                        valid.push(1);
                    } else {
                        values.push(0.0);
                        valid.push(0);
                    }
                }
                (ColumnBuilder::Bool { values, valid }, v) => {
                    if let Some(MetaValue::Bool(x)) = v {
                        values.push(x as u8);
                        valid.push(1);
                    } else {
                        values.push(0);
                        valid.push(0);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn row_count(&self) -> usize {
        match self.columns.first() {
            Some(ColumnBuilder::Str { valid, .. })
            | Some(ColumnBuilder::Int { valid, .. })
            | Some(ColumnBuilder::Float { valid, .. })
            | Some(ColumnBuilder::Bool { valid, .. }) => valid.len(),
            None => 0,
        }
    }

    pub fn finish(self) -> MetaTable {
        MetaTable {
            names: self.schema.fields.iter().map(|(n, _)| n.clone()).collect(),
            types: self.schema.fields.iter().map(|(_, t)| t.code()).collect(),
            columns: self
                .columns
                .into_iter()
                .map(|c| match c {
                    ColumnBuilder::Str { ends, bytes, valid } => {
                        MetaColumn::Str { ends, bytes, valid }
                    }
                    ColumnBuilder::Int { values, valid } => MetaColumn::Int { values, valid },
                    ColumnBuilder::Float { values, valid } => MetaColumn::Float { values, valid },
                    ColumnBuilder::Bool { values, valid } => MetaColumn::Bool { values, valid },
                })
                .collect(),
        }
    }
}

/// Read row `row` of an archived column. Returns `None` for null.
pub fn archived_value(col: &ArchivedMetaColumn, row: usize) -> Option<MetaRef<'_>> {
    match col {
        ArchivedMetaColumn::Str { ends, bytes, valid } => {
            if valid.get(row).map(|v| *v == 1) != Some(true) {
                return None;
            }
            let end = ends[row].to_native() as usize;
            let start = if row == 0 {
                0
            } else {
                ends[row - 1].to_native() as usize
            };
            std::str::from_utf8(&bytes[start..end])
                .ok()
                .map(MetaRef::Str)
        }
        ArchivedMetaColumn::Int { values, valid } => {
            if valid.get(row).map(|v| *v == 1) != Some(true) {
                return None;
            }
            Some(MetaRef::Int(values[row].to_native()))
        }
        ArchivedMetaColumn::Float { values, valid } => {
            if valid.get(row).map(|v| *v == 1) != Some(true) {
                return None;
            }
            Some(MetaRef::Float(values[row].to_native()))
        }
        ArchivedMetaColumn::Bool { values, valid } => {
            if valid.get(row).map(|v| *v == 1) != Some(true) {
                return None;
            }
            Some(MetaRef::Bool(values[row] != 0))
        }
    }
}

/// Validate an archived table's internal consistency after rkyv access-validation:
/// recognized type codes, parallel lengths, equal row counts across columns, and
/// monotonic string offsets in range. Called once on open, so per-row reads can index
/// without re-checking.
pub fn validate_archived(table: &ArchivedMetaTable, doc_count: usize) -> Result<(), ArchiveErr> {
    let n_fields = table.names.len();
    if table.types.len() != n_fields || table.columns.len() != n_fields {
        return Err(ArchiveErr::Validate(
            "metadata table names/types/columns lengths differ".into(),
        ));
    }
    for t in table.types.iter() {
        if MetaType::from_code(*t).is_none() {
            return Err(ArchiveErr::Validate(format!(
                "unknown metadata type code {t}"
            )));
        }
    }
    for (name, col) in table.names.iter().zip(table.columns.iter()) {
        let rows = match col {
            ArchivedMetaColumn::Str { ends, bytes, valid } => {
                if ends.len() != valid.len() {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata column '{name}': offsets/validity length mismatch"
                    )));
                }
                let mut prev = 0u32;
                for end in ends.iter() {
                    let end = end.to_native();
                    if end < prev || end as usize > bytes.len() {
                        return Err(ArchiveErr::Validate(format!(
                            "metadata column '{name}': string offsets not monotonic/in range"
                        )));
                    }
                    prev = end;
                }
                valid.len()
            }
            ArchivedMetaColumn::Int { values, valid } => {
                if values.len() != valid.len() {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata column '{name}': values/validity length mismatch"
                    )));
                }
                valid.len()
            }
            ArchivedMetaColumn::Float { values, valid } => {
                if values.len() != valid.len() {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata column '{name}': values/validity length mismatch"
                    )));
                }
                valid.len()
            }
            ArchivedMetaColumn::Bool { values, valid } => {
                if values.len() != valid.len() {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata column '{name}': values/validity length mismatch"
                    )));
                }
                valid.len()
            }
        };
        if rows != doc_count {
            return Err(ArchiveErr::Validate(format!(
                "metadata column '{name}' has {rows} rows, archive has {doc_count} documents"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> MetaSchema {
        MetaSchema {
            fields: vec![
                ("url".into(), MetaType::Str),
                ("status".into(), MetaType::Int),
                ("score".into(), MetaType::Float),
                ("ok".into(), MetaType::Bool),
            ],
        }
    }

    #[test]
    fn round_trip_with_nulls() {
        let mut b = MetaTableBuilder::new(schema());
        b.push_row(vec![
            Some(MetaValue::Str("https://a".into())),
            Some(MetaValue::Int(200)),
            None,
            Some(MetaValue::Bool(true)),
        ])
        .unwrap();
        b.push_row(vec![None, None, Some(MetaValue::Float(0.5)), None])
            .unwrap();
        let table = b.finish();
        assert_eq!(table.row_count(), 2);

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&table).unwrap();
        let archived = rkyv::access::<ArchivedMetaTable, rkyv::rancor::Error>(&bytes).unwrap();
        validate_archived(archived, 2).unwrap();

        assert_eq!(
            archived_value(&archived.columns[0], 0),
            Some(MetaRef::Str("https://a"))
        );
        assert_eq!(archived_value(&archived.columns[0], 1), None);
        assert_eq!(
            archived_value(&archived.columns[1], 0),
            Some(MetaRef::Int(200))
        );
        assert_eq!(
            archived_value(&archived.columns[2], 1),
            Some(MetaRef::Float(0.5))
        );
        assert_eq!(
            archived_value(&archived.columns[3], 0),
            Some(MetaRef::Bool(true))
        );
        assert_eq!(archived_value(&archived.columns[3], 1), None);
    }

    #[test]
    fn type_mismatch_rejected() {
        let mut b = MetaTableBuilder::new(schema());
        let err = b
            .push_row(vec![Some(MetaValue::Int(1)), None, None, None])
            .unwrap_err();
        assert!(err.to_string().contains("'url' is str, got int"), "{err}");
    }

    #[test]
    fn wrong_arity_rejected() {
        let mut b = MetaTableBuilder::new(schema());
        assert!(b.push_row(vec![None]).is_err());
    }

    #[test]
    fn row_count_mismatch_detected() {
        let mut b = MetaTableBuilder::new(schema());
        b.push_row(vec![None, None, None, None]).unwrap();
        let table = b.finish();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&table).unwrap();
        let archived = rkyv::access::<ArchivedMetaTable, rkyv::rancor::Error>(&bytes).unwrap();
        assert!(validate_archived(archived, 5).is_err());
    }
}
