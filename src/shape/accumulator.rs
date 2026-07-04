use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::config::SamplingConfig;
use crate::error::Result;
use crate::refs::resolver::ResolvedObjectContext;
use crate::scan::path::SourcePath;
use crate::shape::id::ShapeFacts;
use crate::shape::sample::{ObjectSampleAccumulator, ObjectSampleRow};

#[derive(Debug, Default)]
pub struct ShapeAccumulator {
    shapes: HashMap<String, ShapeRow>,
    object_samples: ObjectSampleAccumulator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeRow {
    pub shape_id: String,
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_set_hash: String,
    pub type_set_hash: String,
    pub field_set_json: String,
    pub type_set_json: String,
    pub object_count: u64,
    pub first_seen_document_index: Option<u64>,
    pub first_seen_path: Option<String>,
}

impl ShapeAccumulator {
    pub fn observe_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        context: &ResolvedObjectContext,
        object: &Map<String, Value>,
        config: &SamplingConfig,
    ) -> Result<()> {
        let _ = self.observe_object_with_facts(document_index, path, context, object, config)?;
        Ok(())
    }

    pub fn observe_object_with_facts(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        context: &ResolvedObjectContext,
        object: &Map<String, Value>,
        config: &SamplingConfig,
    ) -> Result<ShapeFacts> {
        let source_path = path.as_str();
        let facts = crate::shape::id::compute_shape_facts(
            &context.canonical_path,
            context.site_path.as_deref(),
            &context.schema_path,
            object,
        );

        let row = self
            .shapes
            .entry(facts.shape_id.clone())
            .or_insert_with(|| ShapeRow {
                shape_id: facts.shape_id.clone(),
                canonical_path: context.canonical_path.clone(),
                site_path: context.site_path.clone(),
                schema_path: context.schema_path.clone(),
                field_set_hash: facts.field_set_hash.clone(),
                type_set_hash: facts.type_set_hash.clone(),
                field_set_json: facts.field_set_json.clone(),
                type_set_json: facts.type_set_json.clone(),
                object_count: 0,
                first_seen_document_index: Some(document_index),
                first_seen_path: Some(source_path.clone()),
            });
        row.object_count += 1;

        self.object_samples.observe(
            document_index,
            &source_path,
            context,
            &facts,
            object,
            config,
        )?;
        Ok(facts)
    }

    pub fn shape_rows(&self) -> Vec<ShapeRow> {
        let mut rows: Vec<_> = self.shapes.values().cloned().collect();
        rows.sort_by(|left, right| left.shape_id.cmp(&right.shape_id));
        rows
    }

    pub fn drain_shape_rows(&mut self) -> Vec<ShapeRow> {
        let mut rows: Vec<_> = self.shapes.drain().map(|(_, row)| row).collect();
        rows.sort_by(|left, right| left.shape_id.cmp(&right.shape_id));
        rows
    }

    pub fn drain_object_sample_rows(&mut self) -> Vec<ObjectSampleRow> {
        self.object_samples.drain_rows()
    }

    pub fn shape_row_count(&self) -> usize {
        self.shapes.len()
    }

    pub fn pending_object_sample_count(&self) -> usize {
        self.object_samples.pending_row_count()
    }
}
