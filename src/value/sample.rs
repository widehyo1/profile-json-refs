use std::collections::HashSet;

use serde_json::Value;

use crate::config::ProfileConfig;
use crate::sketch::priority::PrioritySampler;
use crate::value::identity::value_hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ValueSampleKind {
    FirstSeen,
    FirstNonEmpty,
    PrioritySample,
    HeavyHitterContext,
}

impl ValueSampleKind {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            ValueSampleKind::FirstSeen => "first_seen",
            ValueSampleKind::FirstNonEmpty => "first_non_empty",
            ValueSampleKind::PrioritySample => "priority_sample",
            ValueSampleKind::HeavyHitterContext => "heavy_hitter_context",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueSampleRow {
    pub value_sample_id: String,
    pub field_profile_id: String,
    pub value_hash: Option<String>,
    pub sample_kind: ValueSampleKind,
    pub document_index: u64,
    pub source_path: String,
    pub value_json: Option<String>,
    pub value_json_truncated: bool,
    pub parent_object_json: Option<String>,
    pub parent_object_json_truncated: bool,
    pub sample_priority: Option<u64>,
    pub sample_rank: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ValueSampleAccumulator {
    priority_limit: usize,
    seen_once: HashSet<ValueSampleKind>,
    priority: PrioritySampler<ValueSampleRow>,
    rows: Vec<ValueSampleRow>,
}

impl ValueSampleAccumulator {
    pub fn new(priority_limit: usize, _heavy_hitter_context_limit: usize) -> Self {
        Self {
            priority_limit,
            seen_once: HashSet::new(),
            priority: PrioritySampler::new(priority_limit),
            rows: Vec::new(),
        }
    }

    pub fn observe(
        &mut self,
        document_index: u64,
        source_path: &str,
        field_profile_id: &str,
        value: &Value,
        parent_object: &Value,
        config: &ProfileConfig,
    ) {
        let observation = ValueSampleObservation {
            document_index,
            source_path,
            field_profile_id,
            value,
            parent_object,
            config,
        };

        if self.seen_once.insert(ValueSampleKind::FirstSeen) {
            self.rows.push(make_value_sample_row(
                ValueSampleKind::FirstSeen,
                &observation,
                None,
            ));
        }

        if crate::shape::sample::value_is_non_empty(value)
            && self.seen_once.insert(ValueSampleKind::FirstNonEmpty)
        {
            self.rows.push(make_value_sample_row(
                ValueSampleKind::FirstNonEmpty,
                &observation,
                None,
            ));
        }

        if self.priority_limit > 0 {
            let priority = sample_priority(field_profile_id, document_index, source_path);
            if self.priority.should_accept(priority) {
                self.priority.push(
                    priority,
                    make_value_sample_row(
                        ValueSampleKind::PrioritySample,
                        &observation,
                        Some(priority),
                    ),
                );
            }
        }
    }

    pub fn rows(&self) -> Vec<ValueSampleRow> {
        let mut rows = self.rows.clone();
        for ranked in self.priority.ranked() {
            let mut row = ranked.value;
            row.sample_rank = Some(ranked.rank);
            rows.push(row);
        }

        rows.sort_by(|left, right| {
            left.sample_kind
                .cmp(&right.sample_kind)
                .then_with(|| left.sample_rank.cmp(&right.sample_rank))
                .then_with(|| left.document_index.cmp(&right.document_index))
                .then_with(|| left.source_path.cmp(&right.source_path))
                .then_with(|| left.value_sample_id.cmp(&right.value_sample_id))
        });
        rows
    }

    pub fn drain_rows(&mut self) -> Vec<ValueSampleRow> {
        let rows = self.rows();
        self.rows.clear();
        self.priority.clear();
        rows
    }

    pub fn pending_row_count(&self) -> usize {
        self.rows.len() + self.priority.len()
    }
}

pub fn sample_priority(field_profile_id: &str, document_index: u64, source_path: &str) -> u64 {
    let input = format!("{field_profile_id}\x1f{document_index}\x1f{source_path}");
    sqlite_priority(crate::util::hash::stable_u64(input.as_bytes()))
}

fn sqlite_priority(value: u64) -> u64 {
    value & i64::MAX as u64
}

struct ValueSampleObservation<'a> {
    document_index: u64,
    source_path: &'a str,
    field_profile_id: &'a str,
    value: &'a Value,
    parent_object: &'a Value,
    config: &'a ProfileConfig,
}

fn make_value_sample_row(
    kind: ValueSampleKind,
    observation: &ValueSampleObservation<'_>,
    priority: Option<u64>,
) -> ValueSampleRow {
    let value_json_raw = serde_json::to_string(observation.value).expect("JSON value serializes");
    let (value_json, value_json_truncated) = crate::util::truncate::truncate_utf8(
        &value_json_raw,
        observation.config.sampling.value_json_limit_bytes,
    );
    let parent_json_raw =
        serde_json::to_string(observation.parent_object).expect("JSON parent serializes");
    let (parent_object_json, parent_object_json_truncated) = crate::util::truncate::truncate_utf8(
        &parent_json_raw,
        observation.config.sampling.parent_object_json_limit_bytes,
    );
    let value_hash = value_hash(observation.value);
    let id_input = format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{value_hash}\x1f{}",
        observation.field_profile_id,
        kind.as_sql_str(),
        observation.document_index,
        observation.source_path,
        priority.map(|value| value.to_string()).unwrap_or_default()
    );

    ValueSampleRow {
        value_sample_id: crate::util::hash::stable_hex(id_input.as_bytes()),
        field_profile_id: observation.field_profile_id.to_string(),
        value_hash: Some(value_hash),
        sample_kind: kind,
        document_index: observation.document_index,
        source_path: observation.source_path.to_string(),
        value_json: Some(value_json),
        value_json_truncated,
        parent_object_json: Some(parent_object_json),
        parent_object_json_truncated,
        sample_priority: priority,
        sample_rank: None,
    }
}
