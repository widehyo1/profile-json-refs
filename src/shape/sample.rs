use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::config::SamplingConfig;
use crate::error::Result;
use crate::refs::resolver::ResolvedObjectContext;
use crate::shape::id::ShapeFacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SampleScope {
    CanonicalPath,
    SitePath,
    FieldSet,
    TypeSet,
}

impl SampleScope {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            SampleScope::CanonicalPath => "canonical_path",
            SampleScope::SitePath => "site_path",
            SampleScope::FieldSet => "field_set",
            SampleScope::TypeSet => "type_set",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectSampleKind {
    FirstSeen,
    FirstNonEmpty,
    PrioritySample,
}

impl ObjectSampleKind {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            ObjectSampleKind::FirstSeen => "first_seen",
            ObjectSampleKind::FirstNonEmpty => "first_non_empty",
            ObjectSampleKind::PrioritySample => "priority_sample",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSampleRow {
    pub object_sample_id: String,
    pub sample_scope: SampleScope,
    pub sample_key: String,
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: Option<String>,
    pub field_set_hash: Option<String>,
    pub type_set_hash: Option<String>,
    pub shape_id: Option<String>,
    pub sample_kind: ObjectSampleKind,
    pub document_index: u64,
    pub source_path: String,
    pub sample_json: String,
    pub sample_json_truncated: bool,
    pub sample_is_empty_object: bool,
    pub sample_is_empty_array: bool,
    pub sample_priority: Option<u64>,
    pub sample_rank: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ObjectSampleCandidate {
    pub priority: u64,
    pub row: ObjectSampleRow,
}

#[derive(Debug, Default)]
pub struct ObjectSampleAccumulator {
    once_seen: HashSet<(SampleScope, String, ObjectSampleKind)>,
    priority: HashMap<(SampleScope, String), TopK>,
    pending_rows: Vec<ObjectSampleRow>,
}

impl ObjectSampleAccumulator {
    pub fn observe(
        &mut self,
        document_index: u64,
        source_path: &str,
        context: &ResolvedObjectContext,
        shape: &ShapeFacts,
        object: &Map<String, Value>,
        config: &SamplingConfig,
    ) -> Result<()> {
        let observation = SampleObservation {
            document_index,
            source_path,
            context,
            shape,
            object,
            config,
        };

        for scope in [
            SampleScope::CanonicalPath,
            SampleScope::SitePath,
            SampleScope::FieldSet,
            SampleScope::TypeSet,
        ] {
            let key = object_sample_key(
                scope,
                &context.canonical_path,
                context.site_path.as_deref(),
                Some(&shape.field_set_hash),
                Some(&shape.type_set_hash),
            );

            self.enqueue_once(scope, &key, ObjectSampleKind::FirstSeen, &observation)?;

            if object_is_structurally_non_empty(object) {
                self.enqueue_once(scope, &key, ObjectSampleKind::FirstNonEmpty, &observation)?;
            }

            self.enqueue_priority(scope, &key, &observation)?;
        }
        Ok(())
    }

    pub fn drain_rows(&mut self) -> Vec<ObjectSampleRow> {
        let mut rows = Vec::new();
        rows.append(&mut self.pending_rows);

        let mut priority_keys: Vec<_> = self.priority.keys().cloned().collect();
        priority_keys.sort();
        for key in priority_keys {
            if let Some(top_k) = self.priority.get(&key) {
                rows.extend(top_k.ranked_rows());
            }
        }

        self.once_seen.clear();
        self.priority.clear();
        sort_rows(&mut rows);
        rows
    }

    fn enqueue_once(
        &mut self,
        scope: SampleScope,
        key: &str,
        kind: ObjectSampleKind,
        observation: &SampleObservation<'_>,
    ) -> Result<()> {
        let once_key = (scope, key.to_string(), kind);
        if !self.once_seen.insert(once_key) {
            return Ok(());
        }
        let row = make_object_sample_row(scope, key, kind, observation, None)?;
        self.pending_rows.push(row);
        Ok(())
    }

    fn enqueue_priority(
        &mut self,
        scope: SampleScope,
        key: &str,
        observation: &SampleObservation<'_>,
    ) -> Result<()> {
        let limit = priority_limit(scope, observation.config);
        if limit == 0 {
            return Ok(());
        }

        let priority = sample_priority(
            scope,
            key,
            observation.document_index,
            observation.source_path,
        );
        let row = make_object_sample_row(
            scope,
            key,
            ObjectSampleKind::PrioritySample,
            observation,
            Some(priority),
        )?;
        self.priority
            .entry((scope, key.to_string()))
            .or_insert_with(|| TopK::new(limit))
            .push(ObjectSampleCandidate { priority, row });
        Ok(())
    }
}

struct SampleObservation<'a> {
    document_index: u64,
    source_path: &'a str,
    context: &'a ResolvedObjectContext,
    shape: &'a ShapeFacts,
    object: &'a Map<String, Value>,
    config: &'a SamplingConfig,
}

pub fn object_sample_key(
    scope: SampleScope,
    canonical_path: &str,
    site_path: Option<&str>,
    field_set_hash: Option<&str>,
    type_set_hash: Option<&str>,
) -> String {
    match scope {
        SampleScope::CanonicalPath => canonical_path.to_string(),
        SampleScope::SitePath => format!("{canonical_path}\x1f{}", site_path.unwrap_or("")),
        SampleScope::FieldSet => format!(
            "{canonical_path}\x1f{}\x1f{}",
            site_path.unwrap_or(""),
            field_set_hash.unwrap_or("")
        ),
        SampleScope::TypeSet => format!(
            "{canonical_path}\x1f{}\x1f{}\x1f{}",
            site_path.unwrap_or(""),
            field_set_hash.unwrap_or(""),
            type_set_hash.unwrap_or("")
        ),
    }
}

pub fn object_is_structurally_non_empty(object: &Map<String, Value>) -> bool {
    !object.is_empty()
}

pub fn value_is_non_empty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Object(object) => !object.is_empty(),
        Value::Array(array) => !array.is_empty(),
        Value::String(_) | Value::Bool(_) | Value::Number(_) => true,
    }
}

pub fn sample_priority(
    sample_scope: SampleScope,
    sample_key: &str,
    document_index: u64,
    source_path: &str,
) -> u64 {
    let input = format!(
        "{}\x1f{sample_key}\x1f{document_index}\x1f{source_path}",
        sample_scope.as_sql_str()
    );
    crate::util::hash::stable_u64(input.as_bytes())
}

fn make_object_sample_row(
    scope: SampleScope,
    key: &str,
    kind: ObjectSampleKind,
    observation: &SampleObservation<'_>,
    priority: Option<u64>,
) -> Result<ObjectSampleRow> {
    let sample_value = Value::Object(observation.object.clone());
    let sample_json = serde_json::to_string(&sample_value)?;
    let (sample_json, sample_json_truncated) = crate::util::truncate::truncate_utf8(
        &sample_json,
        observation.config.object_json_limit_bytes,
    );
    let id_input = format!(
        "{}\x1f{key}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        scope.as_sql_str(),
        kind.as_sql_str(),
        observation.document_index,
        observation.source_path,
        priority.map(|value| value.to_string()).unwrap_or_default(),
        observation.shape.shape_id
    );

    Ok(ObjectSampleRow {
        object_sample_id: crate::util::hash::stable_hex(id_input.as_bytes()),
        sample_scope: scope,
        sample_key: key.to_string(),
        canonical_path: observation.context.canonical_path.clone(),
        site_path: observation.context.site_path.clone(),
        schema_path: Some(observation.context.schema_path.clone()),
        field_set_hash: Some(observation.shape.field_set_hash.clone()),
        type_set_hash: Some(observation.shape.type_set_hash.clone()),
        shape_id: Some(observation.shape.shape_id.clone()),
        sample_kind: kind,
        document_index: observation.document_index,
        source_path: observation.source_path.to_string(),
        sample_json,
        sample_json_truncated,
        sample_is_empty_object: observation.object.is_empty(),
        sample_is_empty_array: false,
        sample_priority: priority,
        sample_rank: None,
    })
}

fn priority_limit(scope: SampleScope, config: &SamplingConfig) -> usize {
    match scope {
        SampleScope::CanonicalPath => config.canonical_priority_limit,
        SampleScope::SitePath => config.site_priority_limit,
        SampleScope::FieldSet => config.field_set_priority_limit,
        SampleScope::TypeSet => config.type_set_priority_limit,
    }
}

#[derive(Debug)]
struct TopK {
    limit: usize,
    candidates: Vec<ObjectSampleCandidate>,
}

impl TopK {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            candidates: Vec::with_capacity(limit),
        }
    }

    fn push(&mut self, candidate: ObjectSampleCandidate) {
        if self.candidates.len() < self.limit {
            self.candidates.push(candidate);
            return;
        }

        let Some((worst_index, worst_candidate)) = self
            .candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| compare_candidates(left, right))
        else {
            return;
        };

        if compare_candidates(&candidate, worst_candidate).is_lt() {
            self.candidates[worst_index] = candidate;
        }
    }

    fn ranked_rows(&self) -> Vec<ObjectSampleRow> {
        let mut candidates = self.candidates.clone();
        candidates.sort_by(compare_candidates);
        candidates
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let mut row = candidate.row;
                row.sample_rank = Some((index + 1) as u32);
                row
            })
            .collect()
    }
}

fn compare_candidates(left: &ObjectSampleCandidate, right: &ObjectSampleCandidate) -> Ordering {
    left.priority
        .cmp(&right.priority)
        .then_with(|| left.row.document_index.cmp(&right.row.document_index))
        .then_with(|| left.row.source_path.cmp(&right.row.source_path))
        .then_with(|| left.row.object_sample_id.cmp(&right.row.object_sample_id))
}

fn sort_rows(rows: &mut [ObjectSampleRow]) {
    rows.sort_by(|left, right| {
        left.sample_scope
            .cmp(&right.sample_scope)
            .then_with(|| left.sample_key.cmp(&right.sample_key))
            .then_with(|| left.sample_kind.cmp(&right.sample_kind))
            .then_with(|| left.sample_rank.cmp(&right.sample_rank))
            .then_with(|| left.document_index.cmp(&right.document_index))
            .then_with(|| left.source_path.cmp(&right.source_path))
            .then_with(|| left.object_sample_id.cmp(&right.object_sample_id))
    });
}
