use std::collections::HashMap;

use serde_json::Value;

use crate::util::json_type::JsonType;
use crate::value::identity::{ValueKey, value_hash_from_key, value_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountMethod {
    Exact,
    Approximate,
    Sampled,
    Unavailable,
}

impl CountMethod {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            CountMethod::Exact => "exact",
            CountMethod::Approximate => "approximate",
            CountMethod::Sampled => "sampled",
            CountMethod::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    ExactFull,
    ExactSelected,
    HeavyHitter,
    Sampled,
}

impl ValueSource {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            ValueSource::ExactFull => "exact_full",
            ValueSource::ExactSelected => "exact_selected",
            ValueSource::HeavyHitter => "heavy_hitter",
            ValueSource::Sampled => "sampled",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldValueRow {
    pub field_profile_id: String,
    pub value_hash: String,
    pub value_type: JsonType,
    pub value_text: Option<String>,
    pub value_text_truncated: bool,
    pub count: Option<u64>,
    pub count_method: CountMethod,
    pub value_source: ValueSource,
    pub rank: Option<u32>,
    pub is_complete_distribution: bool,
}

#[derive(Debug, Clone)]
pub struct ExactCounter {
    enabled: bool,
    counts: HashMap<ValueKey, ExactValueState>,
    distinct_threshold: usize,
    byte_budget: usize,
    used_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ExactValueState {
    pub count: u64,
    pub value_type: JsonType,
    pub stored_value: Option<Value>,
    pub approx_bytes: usize,
}

impl ExactCounter {
    pub fn new(distinct_threshold: usize, byte_budget: usize) -> Self {
        Self {
            enabled: true,
            counts: HashMap::new(),
            distinct_threshold,
            byte_budget,
            used_bytes: 0,
        }
    }

    pub fn observe(&mut self, value: &Value) {
        if !self.enabled {
            return;
        }

        let key = value_key(value);
        if let Some(entry) = self.counts.get_mut(&key) {
            entry.count += 1;
            return;
        }

        let approx_bytes = approximate_value_bytes(value);
        self.used_bytes += approx_bytes;
        self.counts.insert(
            key,
            ExactValueState {
                count: 1,
                value_type: JsonType::from_value(value),
                stored_value: Some(value.clone()),
                approx_bytes,
            },
        );

        if self.counts.len() > self.distinct_threshold || self.used_bytes > self.byte_budget {
            self.disable();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn distinct_count(&self) -> Option<u64> {
        self.enabled.then_some(self.counts.len() as u64)
    }

    pub fn exact_full_rows(
        &self,
        field_profile_id: &str,
        value_text_limit: usize,
    ) -> Vec<FieldValueRow> {
        if !self.enabled {
            return Vec::new();
        }

        let mut rows: Vec<_> = self
            .counts
            .iter()
            .map(|(key, state)| {
                let display = state
                    .stored_value
                    .as_ref()
                    .map(|value| crate::value::display::value_text(value, value_text_limit));
                FieldValueRow {
                    field_profile_id: field_profile_id.to_string(),
                    value_hash: value_hash_from_key(key),
                    value_type: state.value_type,
                    value_text: display.as_ref().and_then(|value| value.text.clone()),
                    value_text_truncated: display.is_some_and(|value| value.truncated),
                    count: Some(state.count),
                    count_method: CountMethod::Exact,
                    value_source: ValueSource::ExactFull,
                    rank: None,
                    is_complete_distribution: true,
                }
            })
            .collect();
        rows.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.value_hash.cmp(&right.value_hash))
        });
        for (index, row) in rows.iter_mut().enumerate() {
            row.rank = Some((index + 1) as u32);
        }
        rows
    }

    fn disable(&mut self) {
        self.enabled = false;
        self.counts.clear();
        self.used_bytes = 0;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalExactBudget {
    pub limit_bytes: usize,
    pub used_bytes: usize,
}

impl GlobalExactBudget {
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            limit_bytes,
            used_bytes: 0,
        }
    }

    pub fn try_reserve(&mut self, bytes: usize) -> bool {
        match self.used_bytes.checked_add(bytes) {
            Some(next) if next <= self.limit_bytes => {
                self.used_bytes = next;
                true
            }
            _ => false,
        }
    }
}

pub fn approximate_value_bytes(value: &Value) -> usize {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(number) => number.to_string().len(),
        Value::String(text) => text.len(),
        Value::Array(_) | Value::Object(_) => crate::value::identity::canonical_json(value).len(),
    }
}
