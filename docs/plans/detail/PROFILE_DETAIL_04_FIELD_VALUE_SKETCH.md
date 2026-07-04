# Profile Detail 04: Field Profiles, Value Identity, Exact Distribution, and Sketches

Covers:

```text
Phase 6: field/value accumulators
Phase 7: bounded exact distribution
Phase 8: sketches and priority sampling
```

---

## 1. Target Files

```text
src/field/mod.rs
src/field/id.rs
src/field/accumulator.rs
src/field/summary.rs
src/value/mod.rs
src/value/identity.rs
src/value/interner.rs
src/value/exact_counter.rs
src/value/display.rs
src/sketch/mod.rs
src/sketch/hll.rs
src/sketch/space_saving.rs
src/sketch/priority.rs
tests/field_summary.rs
tests/value_identity.rs
tests/exact_distribution.rs
tests/sketches.rs
tests/value_samples.rs
```

---

## 2. Field Profile Identity

Field profile grain:

```text
shape_id + field_name + observed_type
```

`src/field/id.rs`:

```rust
pub fn field_profile_id(shape_id: &str, field_name: &str, observed_type: crate::util::json_type::JsonType) -> String {
    let input = format!("{shape_id}\x1f{field_name}\x1f{}", observed_type.as_sql_str());
    crate::util::hash::stable_hex(input.as_bytes())
}
```

Same field name across different shapes gets different `field_profile_id`.

Same field name in the same shape but different observed type gets different `field_profile_id`.

---

## 3. Field Summary Counters

`src/field/summary.rs`:

```rust
#[derive(Debug, Default, Clone)]
pub struct FieldSummary {
    pub field_profile_id: String,
    pub profiled_count: u64,
    pub null_count: u64,
    pub non_null_count: u64,
    pub empty_object_count: u64,
    pub empty_array_count: u64,
    pub empty_string_count: u64,
    pub distinct_count: Option<u64>,
    pub distinct_count_method: DistinctCountMethod,
    pub distinct_algorithm: Option<DistinctAlgorithm>,
    pub distinct_error_rate: Option<f64>,
    pub stored_value_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinctCountMethod {
    Exact,
    Approximate,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinctAlgorithm {
    HyperLogLog,
}
```

Update rules:

```rust
pub fn update_summary(summary: &mut FieldSummary, value: &serde_json::Value) {
    summary.profiled_count += 1;

    match value {
        serde_json::Value::Null => {
            summary.null_count += 1;
        }
        serde_json::Value::Object(obj) => {
            summary.non_null_count += 1;
            if obj.is_empty() {
                summary.empty_object_count += 1;
            }
        }
        serde_json::Value::Array(arr) => {
            summary.non_null_count += 1;
            if arr.is_empty() {
                summary.empty_array_count += 1;
            }
        }
        serde_json::Value::String(s) => {
            summary.non_null_count += 1;
            if s.is_empty() {
                summary.empty_string_count += 1;
            }
        }
        _ => {
            summary.non_null_count += 1;
        }
    }
}
```

Important semantics:

```text
null:
  does not establish logical field type by itself

{}:
  object type observed, but no child fields

[]:
  array type observed, but no child elements

"":
  string type observed; may be sentinel/default/null-substitute
```

`""` is eligible for `first_non_empty`.

---

## 4. Field Accumulator

`src/field/accumulator.rs`:

```rust
use std::collections::HashMap;

pub struct FieldAccumulator {
    fields: HashMap<String, ShapeFieldRow>,
    summaries: HashMap<String, FieldValueAccumulator>,
}

pub struct ShapeFieldRow {
    pub field_profile_id: String,
    pub shape_id: String,
    pub field_name: String,
    pub observed_type: crate::util::json_type::JsonType,
    pub observed_count: u64,
    pub null_count: u64,
}

impl FieldAccumulator {
    pub fn observe_object_fields(
        &mut self,
        document_index: u64,
        object_source_path: &str,
        shape_id: &str,
        object: &serde_json::Map<String, serde_json::Value>,
        parent_object: &serde_json::Value,
        config: &crate::config::ProfileConfig,
    ) {
        for (field_name, value) in object {
            let observed_type = crate::util::json_type::JsonType::from_value(value);
            let field_profile_id = crate::field::id::field_profile_id(shape_id, field_name, observed_type);

            let row = self.fields.entry(field_profile_id.clone()).or_insert_with(|| ShapeFieldRow {
                field_profile_id: field_profile_id.clone(),
                shape_id: shape_id.to_string(),
                field_name: field_name.clone(),
                observed_type,
                observed_count: 0,
                null_count: 0,
            });

            row.observed_count += 1;
            if matches!(value, serde_json::Value::Null) {
                row.null_count += 1;
            }

            self.summaries
                .entry(field_profile_id.clone())
                .or_insert_with(|| FieldValueAccumulator::new(field_profile_id.clone(), config))
                .observe(document_index, object_source_path, value, parent_object, config);
        }
    }
}
```

---

## 5. Value Identity

Semantic identity:

```text
stable_hash(canonical_json(value))
```

Implementation requirement:

```text
Do not materialize canonical JSON strings for every observed value on the hot path.
```

v0.1.0 pragmatic approach:

```text
- scalar values: hash typed scalar tokens directly
- null/boolean/integer/number/string: no JSON string materialization needed
- object/array: materialize canonical JSON only when value is selected for exact storage, heavy hitter, or sample
- exact counter may keep compact value keys with optional display materialization
```

`src/value/identity.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKey {
    Null,
    Bool(bool),
    Integer(String),
    Number(String),
    String(String),
    ObjectHash(String),
    ArrayHash(String),
}

pub fn value_key(value: &serde_json::Value) -> ValueKey {
    match value {
        serde_json::Value::Null => ValueKey::Null,
        serde_json::Value::Bool(v) => ValueKey::Bool(*v),
        serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => ValueKey::Integer(n.to_string()),
        serde_json::Value::Number(n) => ValueKey::Number(n.to_string()),
        serde_json::Value::String(s) => ValueKey::String(s.clone()),
        serde_json::Value::Object(_) => {
            let canonical = canonical_json(value);
            ValueKey::ObjectHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
        serde_json::Value::Array(_) => {
            let canonical = canonical_json(value);
            ValueKey::ArrayHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
    }
}

pub fn value_hash(value: &serde_json::Value) -> String {
    let key = value_key(value);
    crate::util::hash::stable_hex(format!("{key:?}").as_bytes())
}
```

Later optimization: use interning/tokenized canonical object representation for object/array values.

---

## 6. Value Text Display

`src/value/display.rs`:

```rust
pub struct DisplayValue {
    pub text: Option<String>,
    pub truncated: bool,
}

pub fn value_text(value: &serde_json::Value, limit: usize) -> DisplayValue {
    let raw = match value {
        serde_json::Value::String(s) => s.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string()),
    };

    crate::util::truncate::truncate_utf8(&raw, limit)
}
```

`value_text` is for selected storage rows only.

Do not compute it for every value if exact/HH/sample will not store the row.

---

## 7. Exact Counter

`src/value/exact_counter.rs`:

```rust
use std::collections::HashMap;
use crate::value::identity::ValueKey;

pub struct ExactCounter {
    enabled: bool,
    counts: HashMap<ValueKey, ExactValueState>,
    distinct_threshold: usize,
    byte_budget: usize,
    used_bytes: usize,
}

pub struct ExactValueState {
    pub count: u64,
    pub value_type: crate::util::json_type::JsonType,
    pub stored_value: Option<serde_json::Value>,
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

    pub fn observe(&mut self, value: &serde_json::Value) {
        if !self.enabled {
            return;
        }

        let key = crate::value::identity::value_key(value);
        let approx_bytes = approximate_value_bytes(value);

        let entry = self.counts.entry(key).or_insert_with(|| {
            self.used_bytes += approx_bytes;
            ExactValueState {
                count: 0,
                value_type: crate::util::json_type::JsonType::from_value(value),
                stored_value: Some(value.clone()),
                approx_bytes,
            }
        });

        entry.count += 1;

        if self.counts.len() > self.distinct_threshold || self.used_bytes > self.byte_budget {
            self.enabled = false;
            self.counts.clear();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}
```

Global exact budget should live above individual counters.

```rust
pub struct GlobalExactBudget {
    pub limit_bytes: usize,
    pub used_bytes: usize,
}
```

If global budget is exceeded, disable exact tracking for newly overflowing field profiles. Do not disable HLL or Space-Saving.

---

## 8. HyperLogLog

`src/sketch/hll.rs`:

```rust
pub struct HyperLogLog {
    precision: u8,
    registers: Vec<u8>,
}

impl HyperLogLog {
    pub fn new(precision: u8) -> Self {
        let m = 1usize << precision;
        Self {
            precision,
            registers: vec![0; m],
        }
    }

    pub fn insert_hash(&mut self, hash: u64) {
        let idx = (hash >> (64 - self.precision)) as usize;
        let w = hash << self.precision;
        let rank = w.leading_zeros() as u8 + 1;
        self.registers[idx] = self.registers[idx].max(rank);
    }

    pub fn estimate(&self) -> u64 {
        todo!("implement HLL estimate with small-range correction")
    }

    pub fn relative_error(&self) -> f64 {
        1.04 / ((1u64 << self.precision) as f64).sqrt()
    }
}
```

Recommended `hll_precision = 14`.

---

## 9. Space-Saving

`src/sketch/space_saving.rs`:

```rust
use std::collections::HashMap;

pub struct SpaceSaving<K> {
    limit: usize,
    counters: HashMap<K, Counter>,
}

#[derive(Debug, Clone)]
pub struct Counter {
    pub count: u64,
    pub error: u64,
}

impl<K> SpaceSaving<K>
where
    K: std::hash::Hash + Eq + Clone,
{
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            counters: HashMap::new(),
        }
    }

    pub fn observe(&mut self, key: K) {
        if let Some(c) = self.counters.get_mut(&key) {
            c.count += 1;
            return;
        }

        if self.counters.len() < self.limit {
            self.counters.insert(key, Counter { count: 1, error: 0 });
            return;
        }

        let victim_key = self.counters
            .iter()
            .min_by_key(|(_, c)| c.count)
            .map(|(k, _)| k.clone())
            .expect("limit > 0");

        let victim = self.counters.remove(&victim_key).unwrap();

        self.counters.insert(key, Counter {
            count: victim.count + 1,
            error: victim.count,
        });
    }

    pub fn top(&self) -> Vec<(K, Counter)> {
        let mut rows: Vec<_> = self.counters.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        rows.sort_by(|a, b| b.1.count.cmp(&a.1.count));
        rows
    }
}
```

Counts from Space-Saving are approximate upper-bound candidates. Write `count_method = 'approximate'`.

---

## 10. Value Priority Samples

`src/sketch/priority.rs` should be generic enough for object and value samples.

For value samples:

```text
sample_kind:
  first_seen
  first_non_empty
  priority_sample
  heavy_hitter_context
```

First/non-empty rule:

```rust
pub fn value_is_non_empty(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Object(obj) => !obj.is_empty(),
        serde_json::Value::Array(arr) => !arr.is_empty(),
        serde_json::Value::String(_) => true,
        serde_json::Value::Bool(_) => true,
        serde_json::Value::Number(_) => true,
    }
}
```

`""` is non-empty.

### 10.1 Heavy Hitter Context Policy

`heavy_hitter_context` is disabled by default in `v0.1.0-rc.2`.

Implementation rules:

```text
- update Space-Saving during scan
- do not emit heavy_hitter_context rows during scan
- do not attach context to transient Space-Saving candidates
- when heavy_hitter_context_sample_limit = 0, write no heavy_hitter_context rows
- if enabled later, create context rows only after Space-Saving finalization and only for final surviving heavy hitter values
```

This prevents high-cardinality fields from turning heavy hitter context into a value-context table proportional to distinct value count.

---

## 11. FieldValueAccumulator

```rust
pub struct FieldValueAccumulator {
    pub field_profile_id: String,
    pub summary: crate::field::summary::FieldSummary,
    pub exact: crate::value::exact_counter::ExactCounter,
    pub hll: crate::sketch::hll::HyperLogLog,
    pub heavy_hitters: crate::sketch::space_saving::SpaceSaving<crate::value::identity::ValueKey>,
    pub value_samples: ValueSampleAccumulator,
}

impl FieldValueAccumulator {
    pub fn new(field_profile_id: String, config: &crate::config::ProfileConfig) -> Self {
        Self {
            summary: FieldSummary {
                field_profile_id: field_profile_id.clone(),
                ..Default::default()
            },
            exact: ExactCounter::new(
                config.value_profile.exact_distinct_threshold,
                config.value_profile.exact_value_bytes_per_field_profile,
            ),
            hll: HyperLogLog::new(config.value_profile.hll_precision),
            heavy_hitters: SpaceSaving::new(config.value_profile.heavy_hitter_limit),
            value_samples: ValueSampleAccumulator::new(config.sampling.value_priority_limit_per_field_profile),
            field_profile_id,
        }
    }

    pub fn observe(
        &mut self,
        document_index: u64,
        source_path: &str,
        value: &serde_json::Value,
        parent_object: &serde_json::Value,
        config: &crate::config::ProfileConfig,
    ) {
        crate::field::summary::update_summary(&mut self.summary, value);

        let key = crate::value::identity::value_key(value);
        let hash64 = crate::util::hash::stable_u64(format!("{key:?}").as_bytes());

        self.hll.insert_hash(hash64);
        self.heavy_hitters.observe(key.clone());
        self.exact.observe(value);

        // rc.2: do not emit heavy_hitter_context during scan.
        // Value samples here are first_seen / first_non_empty / priority_sample only.
        self.value_samples.observe(
            document_index,
            source_path,
            &self.field_profile_id,
            value,
            parent_object,
            config,
        );
    }
}
```

---

## 12. Flush Decision

At final flush for each `field_profile_id`:

If exact is still enabled:

```text
prof_field_summary.distinct_count_method = exact
prof_field_summary.distinct_algorithm = NULL
prof_field_summary.distinct_error_rate = NULL
prof_field_summary.distinct_count = exact distinct count

prof_field_value:
  value_source = exact_full
  count_method = exact
  is_complete_distribution = 1
```

If exact is disabled:

```text
prof_field_summary.distinct_count_method = approximate
prof_field_summary.distinct_algorithm = hyperloglog
prof_field_summary.distinct_error_rate = hll.relative_error()
prof_field_summary.distinct_count = hll.estimate()

prof_field_value:
  value_source = heavy_hitter
  count_method = approximate
  is_complete_distribution = 0
```

Priority samples can also write sampled values:

```text
value_source = sampled
count_method = sampled
is_complete_distribution = 0
```

---

## 13. Phase 6 Tests

```text
tests/field_summary.rs
  - null-only field
  - empty-object-only field
  - empty-array-only field
  - empty-string-only field
  - "" increments empty_string_count and non_null_count
  - same field name in different shapes has different field_profile_id
```

---

## 14. Phase 7 Tests

```text
tests/exact_distribution.rs
  - categorical field below threshold writes exact_full
  - is_complete_distribution = 1
  - count_method = exact
  - distinct_count_method = exact
  - crossing exact_distinct_threshold disables exact_full
  - crossing per-field byte budget disables exact_full
```

---

## 15. Phase 8 Tests

```text
tests/sketches.rs
  - HLL estimate is populated for large profile
  - HLL error rate is non-null for approximate profile
  - Space-Saving rows are bounded by heavy_hitter_limit
  - heavy hitter rows use count_method = approximate

tests/value_samples.rs
  - first_seen value sample exists
  - first_non_empty value sample treats "" as non-empty
  - null first_seen followed by "" creates first_non_empty
  - priority_sample rows are bounded
  - heavy_hitter_context rows are 0 by default
  - high-cardinality fields do not produce heavy_hitter_context rows when the limit is 0
```

---

## 16. Commits

Phase 6:

```bash
git add src/field src/value/identity.rs src/value/display.rs tests/field_summary.rs tests/value_identity.rs
git commit -m "feat(field): accumulate shape-specific field profiles"
```

Phase 7:

```bash
git add src/value/exact_counter.rs tests/exact_distribution.rs
git commit -m "feat(value): add bounded exact distributions"
```

Phase 8:

```bash
git add src/sketch src/value tests/sketches.rs tests/value_samples.rs
git commit -m "feat(sketch): add approximate value profiling"
```
