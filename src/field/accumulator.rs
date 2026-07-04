use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::config::ProfileConfig;
use crate::field::summary::{FieldSummary, update_summary};
use crate::util::json_type::JsonType;

#[derive(Debug, Default)]
pub struct FieldAccumulator {
    fields: HashMap<String, ShapeFieldRow>,
    summaries: HashMap<String, FieldSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeFieldRow {
    pub field_profile_id: String,
    pub shape_id: String,
    pub field_name: String,
    pub observed_type: JsonType,
    pub observed_count: u64,
    pub null_count: u64,
}

impl FieldAccumulator {
    pub fn observe_object_fields(
        &mut self,
        _document_index: u64,
        _object_source_path: &str,
        shape_id: &str,
        object: &Map<String, Value>,
        _parent_object: &Value,
        _config: &ProfileConfig,
    ) {
        for (field_name, value) in object {
            let observed_type = JsonType::from_value(value);
            let field_profile_id =
                crate::field::id::field_profile_id(shape_id, field_name, observed_type);

            let field_row = self
                .fields
                .entry(field_profile_id.clone())
                .or_insert_with(|| ShapeFieldRow {
                    field_profile_id: field_profile_id.clone(),
                    shape_id: shape_id.to_string(),
                    field_name: field_name.clone(),
                    observed_type,
                    observed_count: 0,
                    null_count: 0,
                });
            field_row.observed_count += 1;
            if matches!(value, Value::Null) {
                field_row.null_count += 1;
            }

            let summary = self
                .summaries
                .entry(field_profile_id.clone())
                .or_insert_with(|| FieldSummary {
                    field_profile_id,
                    ..FieldSummary::default()
                });
            update_summary(summary, value);
        }
    }

    pub fn shape_field_rows(&self) -> Vec<ShapeFieldRow> {
        let mut rows: Vec<_> = self.fields.values().cloned().collect();
        rows.sort_by(|left, right| left.field_profile_id.cmp(&right.field_profile_id));
        rows
    }

    pub fn field_summaries(&self) -> Vec<FieldSummary> {
        let mut rows: Vec<_> = self.summaries.values().cloned().collect();
        rows.sort_by(|left, right| left.field_profile_id.cmp(&right.field_profile_id));
        rows
    }
}
