use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::config::ProfileConfig;
use crate::field::summary::{DistinctAlgorithm, DistinctCountMethod, FieldSummary, update_summary};
use crate::sketch::hll::HyperLogLog;
use crate::sketch::space_saving::{SpaceSaving, SpaceSavingUpdate};
use crate::util::json_type::JsonType;
use crate::value::exact_counter::{CountMethod, ExactCounter, FieldValueRow, ValueSource};
use crate::value::identity::{ObservedValue, ValueHashTiming, ValueKey, value_hash_from_key};
use crate::value::sample::{PendingRowDelta, ValueSampleAccumulator, ValueSampleRow};

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

#[derive(Debug, Clone)]
pub struct FieldValueAccumulator {
    pub field_profile_id: String,
    pub summary: FieldSummary,
    pub exact: ExactCounter,
    pub hll: HyperLogLog,
    pub heavy_hitters: SpaceSaving<ValueKey>,
    pub value_samples: ValueSampleAccumulator,
    heavy_hitter_values: HashMap<ValueKey, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldProfileOutput {
    pub summary: FieldSummary,
    pub field_values: Vec<FieldValueRow>,
    pub value_samples: Vec<ValueSampleRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldValueObserveTiming {
    pub value_hash_elapsed: Duration,
    pub value_canonicalize_elapsed: Duration,
    pub field_update_elapsed: Duration,
    pub sample_update_elapsed: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldValueObserveStats {
    pub pending_value_sample_delta: PendingRowDelta,
}

impl FieldValueAccumulator {
    pub fn new(field_profile_id: String, config: &ProfileConfig) -> Self {
        Self {
            summary: FieldSummary {
                field_profile_id: field_profile_id.clone(),
                ..FieldSummary::default()
            },
            exact: ExactCounter::new(
                config.value_profile.exact_distinct_threshold,
                config.value_profile.exact_value_bytes_per_field_profile,
            ),
            hll: HyperLogLog::new(config.value_profile.hll_precision),
            heavy_hitters: SpaceSaving::new(config.value_profile.heavy_hitter_limit),
            value_samples: ValueSampleAccumulator::new(
                config.sampling.value_priority_limit_per_field_profile,
                config.sampling.heavy_hitter_context_sample_limit,
            ),
            heavy_hitter_values: HashMap::new(),
            field_profile_id,
        }
    }

    pub fn observe(
        &mut self,
        document_index: u64,
        source_path: &str,
        value: &Value,
        parent_object: &Map<String, Value>,
        config: &ProfileConfig,
    ) -> FieldValueObserveStats {
        self.observe_inner(
            document_index,
            source_path,
            value,
            parent_object,
            config,
            None,
        )
    }

    pub fn observe_with_timing(
        &mut self,
        document_index: u64,
        source_path: &str,
        value: &Value,
        parent_object: &Map<String, Value>,
        config: &ProfileConfig,
        timing: &mut FieldValueObserveTiming,
    ) -> FieldValueObserveStats {
        self.observe_inner(
            document_index,
            source_path,
            value,
            parent_object,
            config,
            Some(timing),
        )
    }

    fn observe_inner(
        &mut self,
        document_index: u64,
        source_path: &str,
        value: &Value,
        parent_object: &Map<String, Value>,
        config: &ProfileConfig,
        mut timing: Option<&mut FieldValueObserveTiming>,
    ) -> FieldValueObserveStats {
        let started = timing.as_ref().map(|_| Instant::now());
        update_summary(&mut self.summary, value);
        if let (Some(timing), Some(started)) = (timing.as_deref_mut(), started) {
            timing.field_update_elapsed += started.elapsed();
        }

        let started = timing.as_ref().map(|_| Instant::now());
        let observed = if let Some(timing) = timing.as_deref_mut() {
            ObservedValue::from_value_with_canonical_timing(
                value,
                &mut timing.value_canonicalize_elapsed,
            )
        } else {
            ObservedValue::from_value(value)
        };
        if let (Some(timing), Some(started)) = (timing.as_deref_mut(), started) {
            timing.value_hash_elapsed += started.elapsed();
        }

        let started = timing.as_ref().map(|_| Instant::now());
        self.hll.insert_hash(observed.stable_hash64);
        match self.heavy_hitters.observe_update(observed.key.clone()) {
            SpaceSavingUpdate::Existing | SpaceSavingUpdate::Inserted => {}
            SpaceSavingUpdate::Replaced { evicted } => {
                self.heavy_hitter_values.remove(&evicted);
            }
        }
        self.exact.observe_keyed(&observed.key, value);

        if self.heavy_hitters.contains_key(&observed.key) {
            self.heavy_hitter_values
                .insert(observed.key.clone(), value.clone());
        }
        if let (Some(timing), Some(started)) = (timing.as_deref_mut(), started) {
            timing.field_update_elapsed += started.elapsed();
        }

        let started = timing.as_ref().map(|_| Instant::now());
        let pending_value_sample_delta = if let Some(timing) = timing.as_deref_mut() {
            let mut hash_timing = ValueHashTiming::default();
            let delta = self.value_samples.observe_keyed_with_timing(
                document_index,
                source_path,
                &self.field_profile_id,
                &observed.key,
                value,
                parent_object,
                config,
                &mut hash_timing,
            );
            timing.value_hash_elapsed += hash_timing.value_hash_elapsed;
            timing.value_canonicalize_elapsed += hash_timing.value_canonicalize_elapsed;
            delta
        } else {
            self.value_samples.observe_keyed(
                document_index,
                source_path,
                &self.field_profile_id,
                &observed.key,
                value,
                parent_object,
                config,
            )
        };
        if let (Some(timing), Some(started)) = (timing, started) {
            timing.sample_update_elapsed += started.elapsed();
        }

        FieldValueObserveStats {
            pending_value_sample_delta,
        }
    }

    pub fn value_sample_rows(&self) -> Vec<ValueSampleRow> {
        self.value_samples.rows()
    }

    pub fn drain_value_sample_rows(&mut self) -> Vec<ValueSampleRow> {
        self.value_samples.drain_rows()
    }

    pub fn pending_value_sample_count(&self) -> usize {
        self.value_samples.pending_row_count()
    }

    pub fn finish(mut self, config: &ProfileConfig) -> FieldProfileOutput {
        let mut field_values = if self.exact.is_enabled() {
            self.summary.distinct_count_method = DistinctCountMethod::Exact;
            self.summary.distinct_algorithm = None;
            self.summary.distinct_error_rate = None;
            self.summary.distinct_count = self.exact.distinct_count();
            self.exact.exact_full_rows(
                &self.field_profile_id,
                config.value_profile.value_text_limit_bytes,
            )
        } else {
            self.summary.distinct_count_method = DistinctCountMethod::Approximate;
            self.summary.distinct_algorithm = Some(DistinctAlgorithm::HyperLogLog);
            self.summary.distinct_error_rate = Some(self.hll.relative_error());
            self.summary.distinct_count = Some(self.hll.estimate());
            self.heavy_hitter_rows(config)
        };
        field_values.sort_by(|left, right| {
            left.rank
                .cmp(&right.rank)
                .then_with(|| left.value_hash.cmp(&right.value_hash))
        });
        self.summary.stored_value_count = field_values.len() as u64;

        FieldProfileOutput {
            summary: self.summary,
            field_values,
            value_samples: self.value_samples.rows(),
        }
    }

    fn heavy_hitter_rows(&self, config: &ProfileConfig) -> Vec<FieldValueRow> {
        self.heavy_hitters
            .top()
            .into_iter()
            .enumerate()
            .map(|(index, (key, counter))| {
                let display = self.heavy_hitter_values.get(&key).map(|value| {
                    crate::value::display::value_text(
                        value,
                        config.value_profile.value_text_limit_bytes,
                    )
                });
                FieldValueRow {
                    field_profile_id: self.field_profile_id.clone(),
                    value_hash: value_hash_from_key(&key),
                    value_type: key.json_type(),
                    value_text: display.as_ref().and_then(|value| value.text.clone()),
                    value_text_truncated: display.is_some_and(|value| value.truncated),
                    count: Some(counter.count),
                    count_method: CountMethod::Approximate,
                    value_source: ValueSource::HeavyHitter,
                    rank: Some((index + 1) as u32),
                    is_complete_distribution: false,
                }
            })
            .collect()
    }
}
