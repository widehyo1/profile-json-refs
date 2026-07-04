# Probabilistic and Bounded Data Structures

`profile-json-refs` profiles real JSON/JSONL inputs where complete exact value distributions may be too expensive. It uses exact counters when bounded and falls back to approximate or sampled facts when necessary.

---

## 1. Goals

```text
- provide exact facts for small field profiles
- provide approximate distinct counts for large field profiles
- retain frequent value candidates
- retain source-backed samples for inspection
- avoid unbounded memory growth
- label every fact by method and source
```

---

## 2. Per-Field Structures

Each `field_profile_id` should update these from the first observed value:

```text
bounded exact counter
HyperLogLog
Space-Saving heavy hitter tracker
deterministic priority sampler
```

This avoids losing early observations when a field crosses the exact threshold later.

---

## 3. Bounded Exact Counter

Purpose:

```text
exact full distribution for small field profiles
```

Recommended defaults:

```text
exact_distinct_threshold: 4096
exact_value_bytes_per_field_profile: 1048576
global_exact_value_bytes_budget: 268435456
```

Exact mode remains valid only while all limits are respected.

Flush when exact remains valid:

```text
prof_field_summary.distinct_count_method = exact
prof_field_value.value_source = exact_full
prof_field_value.count_method = exact
prof_field_value.is_complete_distribution = 1
```

When any threshold is exceeded, exact distribution output for that field profile is abandoned and approximate/sample outputs are used instead.

---

## 4. HyperLogLog

Purpose:

```text
approximate distinct count per field_profile_id
```

Update value:

```text
value_hash
```

Recommended precision:

```text
hll_precision: 14
```

Expected relative error:

```text
1.04 / sqrt(2^14) ≈ 0.81%
```

Flush target:

```text
prof_field_summary.distinct_count
prof_field_summary.distinct_count_method = approximate
prof_field_summary.distinct_algorithm = hyperloglog
prof_field_summary.distinct_error_rate
```

HLL registers are not stored in `profile.sqlite`. The database stores the resulting fact, not mergeable sketch state.

---

## 5. Space-Saving Heavy Hitters

Purpose:

```text
frequent value candidate detection per field_profile_id
```

Recommended default:

```text
heavy_hitter_limit: 128
```

Flush target when exact full distribution is not available:

```text
prof_field_value.value_source = heavy_hitter
prof_field_value.count_method = approximate
prof_field_value.is_complete_distribution = 0
```

Heavy hitter rows are candidate facts, not complete distribution facts.

If exact full distribution is available for a field profile, separate heavy hitter rows are not required because the full distribution can be sorted by exact count.

---

## 6. Array Scope

v0.1.0 does not define array-specific sketches.

HLL, Space-Saving, and deterministic priority sampling apply to shape-specific field profiles, including fields observed inside object elements of arrays.

Dedicated sketches for array length distribution, array element-type distribution, scalar array item distribution, or positional semantics are deferred with any future prof_array_* model.

---

## 7. Deterministic Priority Sampling

Purpose:

```text
bounded, chunk-mergeable representative samples
```

Priority sampling is preferred over long-lived per-key reservoir state because sample key cardinality can be large.

Priority computation should be deterministic:

```text
sample_priority = stable_hash(sample_key + document_index + source_path + local discriminator)
```

The implementation must choose one ordering and use it consistently, for example lower priority value wins.

---

## 8. Object Sample Policy

Object samples are stored in `prof_object_sample` for these sample scopes:

```text
canonical_path
site_path
field_set
type_set
```

For every materialized key:

```text
first_seen:
  mandatory; insert as soon as the key is observed

first_non_empty:
  best-effort; insert as soon as a structurally non-empty sample appears

priority_sample:
  bounded top-K by deterministic priority
```

Recommended priority sample limits:

```text
canonical_path: 1
site_path:      1
field_set:      2
type_set:       4
```

Maximum stored rows per key are therefore:

```text
canonical_path: 3
site_path:      3
field_set:      4
type_set:       6
```

because `first_non_empty` may be absent.

---

## 9. Value Sample Policy

Value/context samples are stored in `prof_field_value_sample`.

Kinds:

```text
first_seen
first_non_empty
priority_sample
heavy_hitter_context
```

Recommended defaults:

```text
priority_sample_limit_per_field_profile: 4
heavy_hitter_context_sample_limit: 0
value_json_limit_bytes: 1024
parent_object_json_limit_bytes: 1024
```

`heavy_hitter_context` is disabled by default.

The Space-Saving sketch should track candidate values during scan, but it must not cause scan-time `heavy_hitter_context` rows to be written. If context sampling is explicitly enabled, context rows are created only for final surviving heavy hitter values after Space-Saving finalization.

Value-level non-empty rules:

```text
null    empty
{}      empty object
[]      empty array
""      non-empty
0       non-empty
false   non-empty
{"a": null} non-empty object
[null] non-empty array
```

---

## 10. Chunk Merge and Prune

Priority samples are maintained as chunk-local top-K candidates.

At chunk flush:

```text
1. insert chunk-local candidates into SQLite
2. rank candidates by priority per key
3. delete rows whose rank exceeds configured limit
4. clear chunk-local memory
```

This bounds both in-memory and persisted priority sample rows.

`first_seen` and `first_non_empty` should use unique insert semantics and do not require long-lived per-key memory.

---

## 11. Count Method Semantics

Allowed `count_method` values:

```text
exact
approximate
sampled
unavailable
```

Semantics:

```text
exact:
  count is exact for the represented scope

approximate:
  count is produced by an approximate or bounded algorithm

sampled:
  row comes from a sample and is not a full count

unavailable:
  count was not computed
```

---

## 12. Distinct Count Method Semantics

Allowed `distinct_count_method` values:

```text
exact
approximate
unavailable
```

Semantics:

```text
exact:
  exact distinct count from bounded exact counter

approximate:
  HyperLogLog estimate

unavailable:
  distinct count not computed
```

---

## 13. Value Source Semantics

Allowed `value_source` values:

```text
exact_full
exact_selected
heavy_hitter
sampled
```

Semantics:

```text
exact_full:
  complete exact distribution entry

exact_selected:
  exact count for a selected value

heavy_hitter:
  frequent value candidate

sampled:
  sampled value observation
```

---

## 14. Storage Limits

Recommended defaults:

```text
value_text_limit_bytes: 512
value_json_limit_bytes: 1024
sample_json_limit_bytes: 16384
parent_object_json_limit_bytes: 1024
```

Truncated stored text/JSON must set the corresponding `*_truncated = 1` flag.
