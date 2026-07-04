# Population Rules

This document defines how source scan events and refs context populate `profile.sqlite`.

---

## 1. High-Level Flow

```text
1. resolve CLI/config
2. open refs/schemas.sqlite
3. validate required refs tables
4. create profile.sqlite tables
5. stream source file
6. resolve structural context for observed objects, including object elements inside arrays when refs context is available
7. update shape, field, value, and sample accumulators
8. flush chunk-local rows to SQLite
9. merge/prune priority samples
10. create indexes
11. write prof_source_summary
12. print stdout summary
```

The implementation must not materialize the full input source.

---

## 2. Refs Context

For each observed object, the scanner should resolve as much of the following as possible:

```text
canonical_path
site_path
schema_path
field_name
observed_type
field combination / presence shape
```

If some refs-side context is unavailable, profile generation should continue and write the facts that can still be anchored reliably.

---

## 3. Shape Accumulation

When an object is observed:

```text
1. resolve canonical_path
2. resolve site_path when available
3. resolve schema_path
4. compute sorted field_set_json
5. compute sorted type_set_json
6. compute field_set_hash
7. compute type_set_hash
8. compute shape_id
9. increment prof_shape.object_count
10. record first_seen_document_index and first_seen_path if absent
11. update object samples for all applicable sample scopes
```

Flush target:

```text
prof_shape
prof_object_sample
```

---

## 4. Heterogeneous Object Array Population

When an array element is an object and refs can resolve structural context for the element, the scanner treats the element object as a normal shaped object.

A single array site may produce multiple prof_shape rows when elements have different field sets or type sets.

The scanner must not collapse heterogeneous object elements into a single array-level profile row.

Dedicated array statistics are not populated in v0.1.0. Deferred facts include:

array length distribution array element-type distribution scalar array item distribution positional semantics nested array-specific profile tables

Array fields themselves are still counted as fields with observed_type = 'array'. Empty array values contribute to empty_array_count in prof_field_summary; this is not an array length distribution.

---

## 5. Object Sampling

Object samples are collected for four navigation grains:

```text
canonical_path:
  sample_key = canonical_path

site_path:
  sample_key = canonical_path + site_path

field_set:
  sample_key = canonical_path + site_path + field_set_hash

type_set:
  sample_key = canonical_path + site_path + field_set_hash + type_set_hash
```

For each materialized sample key:

```text
first_seen:
  required; inserted when the key is first observed

first_non_empty:
  best-effort; inserted when the first structurally non-empty candidate is observed

priority_sample:
  bounded; selected with deterministic priority sampling
```

### 5.1 Empty and Non-Empty Rules

```text
{}      empty object
[]      empty array
null    empty value
""      non-empty
0       non-empty
false   non-empty
{"a": null} non-empty object
[null] non-empty array
```

{}, [], and null are treated as empty for sampling because they do not provide enough structure or logical type evidence for reverse engineering.

An empty string is different. It proves that the observed value is a string and may itself be meaningful as a sentinel/default value. Therefore "" is eligible for first_non_empty

### 5.2 first_seen / first_non_empty Merge

`first_seen` and `first_non_empty` should be written with `INSERT OR IGNORE` semantics against:

```text
sample_scope + sample_key + sample_kind
```

They should not be held in memory for the whole scan.

### 5.3 Priority Sample Merge

Priority samples are chunk-local top-K candidates.

At chunk flush:

```text
1. insert chunk-local priority candidates
2. rank rows by sample_priority within each sample_scope + sample_key
3. delete rows whose rank exceeds the configured limit
```

This keeps both in-memory sample state and SQLite sample rows bounded.

---

## 6. Shape Field Accumulation

For each field in a shaped object:

```text
1. compute observed_type
2. compute field_profile_id = shape_id + field_name + observed_type
3. increment observed_count
4. increment null_count when value is null
5. update field summary counters
6. update exact value counter
7. update HLL
8. update Space-Saving heavy hitter tracker
9. update value samples
```

Flush targets:

```text
prof_shape_field
prof_field_summary
prof_field_value
prof_field_value_sample
```

---

## 7. Field Summary Counters

For each `field_profile_id`, track:

```text
profiled_count
null_count
non_null_count
empty_object_count
empty_array_count
```

Rules:

```text
value is null:
  null_count += 1

value is not null:
  non_null_count += 1

value is {}:
  empty_object_count += 1

value is []:
  empty_array_count += 1
```

---

## 8. Value Identity

Semantic identity:

```text
value_hash = stable_hash(canonical_json(value))
```

Implementation rule:

```text
Do not materialize canonical JSON for every observed value on the hot path.
```

Use typed tokens, compact identities, or interning where practical. Materialize display text only when the value is selected for storage.

---

## 9. Exact and Approximate Value Distribution

Each `field_profile_id` should update all relevant structures from the beginning:

```text
- bounded exact counter
- HyperLogLog
- Space-Saving heavy hitter tracker
- deterministic priority sampler
```

If exact tracking stays within thresholds:

```text
prof_field_summary.distinct_count_method = exact
prof_field_value.value_source = exact_full
prof_field_value.count_method = exact
prof_field_value.is_complete_distribution = 1
```

If exact tracking exceeds thresholds:

```text
prof_field_summary.distinct_count_method = approximate
prof_field_summary.distinct_algorithm = hyperloglog
prof_field_value.value_source = heavy_hitter or sampled
prof_field_value.count_method = approximate or sampled
prof_field_value.is_complete_distribution = 0
```

---

## 10. Value Samples

For each `field_profile_id`, collect:

```text
first_seen
first_non_empty
priority_sample
heavy_hitter_context
```

`first_seen` and `first_non_empty` are source-backed examples. `priority_sample` is bounded by deterministic priority.

`heavy_hitter_context` is optional and disabled by default in `v0.1.0-rc.2`.

The scanner must not write `heavy_hitter_context` rows for transient Space-Saving candidates. When enabled, heavy hitter context rows may only be emitted after Space-Saving finalization and only for final surviving heavy hitter values.

Value sample flush rules mirror object sample flush rules:

```text
- first_seen / first_non_empty can be inserted immediately or at chunk flush with unique keys
- priority samples are chunk-local top-K and SQLite-pruned to global top-K
- heavy_hitter_context rows are skipped when sampling.value.heavy_hitter_context_sample_limit = 0
- when enabled, heavy_hitter_context rows are finalization-only and bounded per final retained heavy hitter value
```

---

## 11. Chunk Flush Policy

Chunk flush is required for sample safety.

Flush triggers may include:

```text
- sample row buffer reaches configured chunk_flush_rows
- value row buffer reaches writer batch size
- memory budget pressure
- end of input
```

At flush:

```text
1. insert first_seen / first_non_empty rows with ignore-on-conflict semantics
2. insert priority sample candidates
3. prune priority samples to per-key limits
4. insert or update shape/field/value aggregate rows
5. clear chunk-local buffers
```

The implementation must not keep unbounded per-key sample state in memory.

---

## 12. prof_source_summary

At end of run, write one row containing:

```text
source format
document count
object count
array count
scalar count
canonical path count
site path count
shape count
field profile count
stored value count
```

This row is also the basis for default stdout.

---

## 13. Warnings

Warnings are printed to stderr and not stored in SQLite.

Warnings must not stop execution when a usable `profile.sqlite` can still be written.
