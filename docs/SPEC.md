# profile-json-refs Specification

Release status: `v0.1.0-rc.2`.

This document defines the current release-candidate contract for `v0.1.0`. If the rc.2 regression and large-input smoke checks pass, this contract becomes the final `v0.1.0` contract.

`profile-json-refs` is a value-level profiling tool for JSON and JSONL snapshots.

It runs downstream of `dump-json-refs`. `dump-json-refs` produces structural refs in `refs/schemas.sqlite`; `profile-json-refs` consumes the original JSON/JSONL source file and `refs/schemas.sqlite`, then writes `profile.sqlite`.

```text
JSON / JSONL source
        │
        ├── dump-json-refs ───────► refs/schemas.sqlite
        │
        └── profile-json-refs ────► profile.sqlite
                 ▲
                 └── consumes refs/schemas.sqlite
```

`profile-json-refs` focuses on one input snapshot at a time. It does not track changes across snapshots.

---

## 1. Responsibility

`profile-json-refs` produces best-effort value-level profile facts.

Inputs:

```text
- original JSON or JSONL source file
- refs/schemas.sqlite produced by dump-json-refs
```

Output:

```text
- profile.sqlite
```

The facts support human inspection and UX presentation.

---

## 2. Non-Responsibility

`profile-json-refs` does not own:

```text
- generating refs
- producing refs/schemas.sqlite
- rendering refs JSON files
- rendering site path symlinks
- accepting stdin in v0.1.0
- generating DBML
- generating SQL DDL
- generating parquet files
- deciding final table boundaries
- deciding primary keys or foreign keys
- tracking lineage across snapshots
```

It must not encode final materialization decisions.

---

## 3. Required refs Contract

`profile-json-refs` v0.1.0 expects `refs/schemas.sqlite` to provide structural facts equivalent to:

```text
schema_paths
array_index_refs
schema_definitions
schema_object_counts
schema_field_counts
schema_site_counts
schema_site_field_counts
schema_site_presence_shapes
schema_site_presence_shape_limits
```

`schema_site_*` tables are structural seeds for profile shape grouping.

If `schema_site_presence_shape_limits.truncated = 1`, profile generation should continue. The source scan may compute profile shapes from the original source, but the implementation must not pretend that every upstream refs presence-shape identity was stored.

---

## 4. One-Shot Artifact Model

`profile.sqlite` is a one-shot result artifact. It is not a run history database.

The database must not contain:

```text
profile_run_id
source_id
prof_run
prof_manifest
prof_algorithm
prof_warning
```

Execution settings belong to CLI arguments or config files. Warnings belong to stderr. The database contains facts only.

---

## 5. CLI Contract

Default command:

```bash
profile-json-refs <INPUT_FILE>
```

Expanded command:

```bash
profile-json-refs <INPUT_FILE> --refs refs/schemas.sqlite --out profile.sqlite
```

Defaults:

```text
--refs refs/schemas.sqlite
--out  profile.sqlite
```

Supported options:

```text
--refs <FILE>
-o, --out <FILE>
--jsonl
--config <FILE>
--shape-sample-limit <N>
--value-sample-limit <N>
--heavy-hitter-limit <N>
--hll-precision <N>
--value-text-limit <BYTES>
--perf-log
--perf-log-file <FILE>
--perf-log-dbstat
--quiet
--help
```

`--strict` is not part of v0.1.0.

---

## 6. Stdin Policy

`profile-json-refs` does not support stdin in v0.1.0.

Supported:

```bash
profile-json-refs data.jsonl --jsonl
```

Unsupported:

```bash
cat data.jsonl | profile-json-refs --jsonl
profile-json-refs - --jsonl
```

Rationale: the tool profiles one source snapshot against `refs/schemas.sqlite`; a named input file keeps the relationship between the source file, refs database, and generated profile artifact explicit.

---

## 7. CLI Output Policy

`profile.sqlite` is the primary result.

Default stdout prints only:

```text
- output path
- prof_source_summary-level summary
- elapsed time
```

Example:

```text
profile-json-refs: wrote profile.sqlite

documents: 10000000
objects: 43281901
arrays: 120331
scalars: 9841123
canonical_paths: 128
site_paths: 412
shapes: 1284
field_profiles: 9731
stored_values: 124000
elapsed: 41.238s
```

`--quiet` suppresses normal stdout on success. Warnings and errors are still written to stderr.

`--perf-log` prints detailed timing and progress events to stderr by default, not stdout.

`--perf-log-file <FILE>` writes perf-log events to a file instead of stderr. The implementation should flush this file during execution so long runs can be diagnosed before completion.

`--perf-log-dbstat` enables optional SQLite `dbstat` diagnostics in perf-log output. Because `dbstat` can be expensive, it is opt-in.

No detailed shape, field, value, or sample rows are printed to stdout. No separate text or JSON report is produced. `profile.sqlite` is the report.

---

## 8. Exit Behavior

```text
success:
  profile.sqlite was written
  exit code 0

usable partial result:
  profile.sqlite was written
  warnings printed to stderr
  exit code 0

fatal failure:
  profile.sqlite could not be written
  non-zero exit code
```

Recoverable warnings must not stop downstream execution.

---

## 9. Physical Table Set

v0.1.0 produces these physical tables:

```text
prof_source_summary
prof_object_sample
prof_shape
prof_shape_field
prof_field_summary
prof_field_value
prof_field_value_sample
```

No view is created in v0.1.0.

---

## 10. Identity Grain

The profile database does not use `source_id` or `profile_run_id`.

### 10.1 shape_id

Recommended grain:

```text
canonical_path
+ site_path
+ schema_path
+ field_set_hash
+ type_set_hash
```

Same canonical path but different field combination creates a different shape. Same field combination but different observed type composition also creates a different shape.

### 10.2 field_profile_id

Recommended grain:

```text
shape_id
+ field_name
+ observed_type
```

### 10.3 value_hash

Semantic identity:

```text
stable_hash(canonical_json(value))
```

Implementation requirement: do not materialize canonical JSON strings for every observed value on the hot path. Use typed value tokens, compact identities, or value interning where practical. Materialize `value_text` only at storage boundaries for selected values, samples, heavy hitter candidates, or exact small-input distributions.

---

## 11. Shape-Aware Navigation Model

Facts should support this access path:

```text
canonical path
  -> site path
    -> field combination
      -> shape with type
        -> object samples
          -> field value profile
```

Object sample grains:

```text
canonical_path
canonical_path + site_path
canonical_path + site_path + field_set_hash
canonical_path + site_path + field_set_hash + type_set_hash
```

`profile-json-refs` preserves the keys for this path but does not own display tree rendering, sort policy, or interactive state.

### 11.1 Heterogeneous Array Policy

`profile-json-refs` v0.1.0 supports heterogeneous object arrays through existing shape profiling.

When array elements are objects and refs provides structural context for those elements, the scanner must profile each object element using the resolved canonical path, site path, schema path, field set, and type set.

A single array site may therefore produce multiple `prof_shape` rows.
This policy applies to object elements only. Scalar-only or mixed scalar/object array-specific profiling is deferred unless scalar values are already represented as ordinary field values through refs context.

`profile-json-refs` v0.1.0 does not provide dedicated array profiling tables. Array-specific facts such as length distribution, element-type distribution, positional semantics, and scalar element distribution are deferred.

Array fields themselves are still recorded as fields with `observed_type = 'array'`, and empty arrays contribute to `empty_array_count`.

---

## 12. Sampling Policy

Sampling has two responsibilities:

```text
1. guarantee that every materialized navigation key has at least one source-backed sample;
2. provide bounded representative samples without unbounded memory growth.
```

Sample kinds:

```text
first_seen:
  mandatory; written when a sample key is first observed

first_non_empty:
  best-effort; written when a structurally non-empty candidate is first observed

priority_sample:
  bounded representative sample chosen by deterministic priority

heavy_hitter_context:
  optional field value context sample for a final surviving heavy hitter value.
  Disabled by default in v0.1.0-rc.2.
```

`first_seen` must not wait for a meaningful value. It prevents sample absence.

`first_non_empty` prevents UX from being stuck with `{}` or `[]` when a later row has structure. If no `first_non_empty` row exists for a key, that absence is itself a fact: no structurally non-empty candidate was observed for that key.

### 12.1 Heavy Hitter Context Policy

`heavy_hitter_context` is disabled by default in `v0.1.0-rc.2`.

The scanner must not emit `heavy_hitter_context` rows for every observed Space-Saving candidate.

When enabled, heavy hitter context samples may only be written for final surviving heavy hitter values after Space-Saving finalization. This prevents high-cardinality fields from turning heavy hitter context into an unbounded value-context sample table.

Default:

```text
sampling.value.heavy_hitter_context_sample_limit = 0
```

Non-empty rules:

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

{}, [], and null are empty for sampling because they do not provide enough structure or logical type evidence.

An empty string still proves `observed_type = string`. It may be meaningful as a sentinel/default value in relational reverse engineering, so it is treated as non-empty.

Object sample defaults:

```text
canonical_path: first_seen 1 + first_non_empty 0..1 + priority_sample 1
site_path:      first_seen 1 + first_non_empty 0..1 + priority_sample 1
field_set:      first_seen 1 + first_non_empty 0..1 + priority_sample 2
type_set:       first_seen 1 + first_non_empty 0..1 + priority_sample 4
```

The implementation must flush sample candidates in chunks. It must not keep unbounded per-key sample state in memory.

Value sample defaults in `v0.1.0-rc.2`:

```text
value_json_limit_bytes: 1024
parent_object_json_limit_bytes: 1024
priority_sample_limit_per_field_profile: 4
heavy_hitter_context_sample_limit: 0
```

---

## 13. Data Model

### 13.1 prof_source_summary

Single summary row. No `id` column.

```sql
CREATE TABLE prof_source_summary (
    source_format TEXT NOT NULL CHECK (source_format IN ('json', 'jsonl', 'unknown')),
    total_document_count INTEGER CHECK (total_document_count >= 0),
    total_object_count INTEGER CHECK (total_object_count >= 0),
    total_array_count INTEGER CHECK (total_array_count >= 0),
    total_scalar_count INTEGER CHECK (total_scalar_count >= 0),
    total_canonical_path_count INTEGER CHECK (total_canonical_path_count >= 0),
    total_site_path_count INTEGER CHECK (total_site_path_count >= 0),
    total_shape_count INTEGER CHECK (total_shape_count >= 0),
    total_field_profile_count INTEGER CHECK (total_field_profile_count >= 0),
    total_stored_value_count INTEGER CHECK (total_stored_value_count >= 0)
);
```

### 13.2 prof_object_sample

Stores object samples for canonical, site, field-set, and type-set navigation grains.

```sql
CREATE TABLE prof_object_sample (
    object_sample_id TEXT PRIMARY KEY,
    sample_scope TEXT NOT NULL CHECK (sample_scope IN ('canonical_path', 'site_path', 'field_set', 'type_set')),
    sample_key TEXT NOT NULL,
    canonical_path TEXT NOT NULL,
    site_path TEXT,
    schema_path TEXT,
    field_set_hash TEXT,
    type_set_hash TEXT,
    shape_id TEXT,
    sample_kind TEXT NOT NULL CHECK (sample_kind IN ('first_seen', 'first_non_empty', 'priority_sample')),
    document_index INTEGER,
    source_path TEXT,
    sample_json TEXT NOT NULL,
    sample_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (sample_json_truncated IN (0, 1)),
    sample_is_empty_object INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_object IN (0, 1)),
    sample_is_empty_array INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_array IN (0, 1)),
    sample_priority INTEGER,
    sample_rank INTEGER
);

CREATE UNIQUE INDEX idx_prof_object_sample_once
ON prof_object_sample(sample_scope, sample_key, sample_kind)
WHERE sample_kind IN ('first_seen', 'first_non_empty');

CREATE INDEX idx_prof_object_sample_key
ON prof_object_sample(sample_scope, sample_key, sample_rank);

CREATE INDEX idx_prof_object_sample_shape
ON prof_object_sample(shape_id, sample_rank);
```

### 13.3 prof_shape

```sql
CREATE TABLE prof_shape (
    shape_id TEXT PRIMARY KEY,
    canonical_path TEXT NOT NULL,
    site_path TEXT,
    schema_path TEXT NOT NULL,
    field_set_hash TEXT NOT NULL,
    type_set_hash TEXT NOT NULL,
    field_set_json TEXT NOT NULL,
    type_set_json TEXT NOT NULL,
    object_count INTEGER NOT NULL CHECK (object_count >= 0),
    first_seen_document_index INTEGER,
    first_seen_path TEXT
);

CREATE INDEX idx_prof_shape_canonical ON prof_shape(canonical_path, object_count DESC);
CREATE INDEX idx_prof_shape_site ON prof_shape(site_path, object_count DESC);
CREATE INDEX idx_prof_shape_schema ON prof_shape(schema_path, object_count DESC);
CREATE INDEX idx_prof_shape_field_set ON prof_shape(field_set_hash);
CREATE INDEX idx_prof_shape_type_set ON prof_shape(type_set_hash);
```

### 13.4 prof_shape_field

```sql
CREATE TABLE prof_shape_field (
    field_profile_id TEXT PRIMARY KEY,
    shape_id TEXT NOT NULL,
    field_name TEXT NOT NULL,
    observed_type TEXT NOT NULL CHECK (observed_type IN ('null','boolean','integer','number','string','object','array','unknown')),
    observed_count INTEGER NOT NULL CHECK (observed_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),
    FOREIGN KEY (shape_id) REFERENCES prof_shape(shape_id)
);

CREATE UNIQUE INDEX idx_prof_shape_field_unique
ON prof_shape_field(shape_id, field_name, observed_type);

CREATE INDEX idx_prof_shape_field_name ON prof_shape_field(field_name);
```

### 13.5 prof_field_summary

```sql
CREATE TABLE prof_field_summary (
    field_profile_id TEXT PRIMARY KEY,
    profiled_count INTEGER NOT NULL CHECK (profiled_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),
    non_null_count INTEGER NOT NULL DEFAULT 0 CHECK (non_null_count >= 0),
    empty_object_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_object_count >= 0),
    empty_array_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_array_count >= 0),
    distinct_count INTEGER CHECK (distinct_count >= 0),
    distinct_count_method TEXT NOT NULL CHECK (distinct_count_method IN ('exact','approximate','unavailable')),
    distinct_algorithm TEXT CHECK (distinct_algorithm IN ('hyperloglog')),
    distinct_error_rate REAL,
    stored_value_count INTEGER NOT NULL DEFAULT 0 CHECK (stored_value_count >= 0),
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);
```

Special cases:

```text
null only:
  profiled_count = null_count

empty object only:
  profiled_count = empty_object_count
  observed_type = object

empty array only:
  profiled_count = empty_array_count
  observed_type = array
```

### 13.6 prof_field_value

```sql
CREATE TABLE prof_field_value (
    field_profile_id TEXT NOT NULL,
    value_hash TEXT NOT NULL,
    value_type TEXT NOT NULL CHECK (value_type IN ('null','boolean','integer','number','string','object','array','unknown')),
    value_text TEXT,
    value_text_truncated INTEGER NOT NULL DEFAULT 0 CHECK (value_text_truncated IN (0, 1)),
    count INTEGER CHECK (count >= 0),
    count_method TEXT NOT NULL CHECK (count_method IN ('exact','approximate','sampled','unavailable')),
    value_source TEXT NOT NULL CHECK (value_source IN ('exact_full','exact_selected','heavy_hitter','sampled')),
    rank INTEGER,
    is_complete_distribution INTEGER NOT NULL DEFAULT 0 CHECK (is_complete_distribution IN (0, 1)),
    PRIMARY KEY (field_profile_id, value_hash, value_source),
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE INDEX idx_prof_field_value_count ON prof_field_value(field_profile_id, count DESC);
CREATE INDEX idx_prof_field_value_hash ON prof_field_value(value_hash);
```

### 13.7 prof_field_value_sample

```sql
CREATE TABLE prof_field_value_sample (
    value_sample_id TEXT PRIMARY KEY,
    field_profile_id TEXT NOT NULL,
    value_hash TEXT,
    sample_kind TEXT NOT NULL CHECK (sample_kind IN ('first_seen','first_non_empty','priority_sample','heavy_hitter_context')),
    document_index INTEGER,
    source_path TEXT,
    value_json TEXT,
    value_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (value_json_truncated IN (0, 1)),
    parent_object_json TEXT,
    parent_object_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (parent_object_json_truncated IN (0, 1)),
    sample_priority INTEGER,
    sample_rank INTEGER,
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE INDEX idx_prof_field_value_sample_field
ON prof_field_value_sample(field_profile_id, sample_rank);

CREATE INDEX idx_prof_field_value_sample_hash
ON prof_field_value_sample(value_hash);
```

---

## 14. Population Rules

High-level scan behavior:

```text
1. load refs structural context
2. stream the source file
3. resolve canonical_path / site_path / schema_path for observed objects
4. compute field_set and type_set
5. update shape and field accumulators
6. update exact counters, HLL, Space-Saving, and priority samplers
7. flush chunk-local rows to SQLite
8. merge/prune priority samples per key
9. write final summaries and indexes
```

`first_seen` and `first_non_empty` samples should be written with `INSERT OR IGNORE` semantics against `(sample_scope, sample_key, sample_kind)`.

Priority samples are chunk-local top-K candidates. At chunk flush, merge them into SQLite and prune each `(sample_scope, sample_key)` or `field_profile_id` to the configured limit.

---

## 15. Exact and Approximate Value Policy

Each `field_profile_id` should update these structures from the beginning:

```text
- bounded exact value counter
- HyperLogLog
- Space-Saving heavy hitter tracker
- deterministic priority sampler
```

If exact tracking remains within threshold:

```text
prof_field_summary.distinct_count_method = exact
prof_field_value.value_source = exact_full
prof_field_value.count_method = exact
prof_field_value.is_complete_distribution = 1
```

If threshold is exceeded:

```text
prof_field_summary.distinct_count_method = approximate
prof_field_summary.distinct_algorithm = hyperloglog
prof_field_value.value_source = heavy_hitter or sampled
prof_field_value.count_method = approximate or sampled
prof_field_value.is_complete_distribution = 0
```

Recommended defaults:

```text
exact_distinct_threshold: 4096
exact_value_bytes_per_field_profile: 1048576
global_exact_value_bytes_budget: 268435456
hll_precision: 14
heavy_hitter_limit: 128
value_text_limit_bytes: 512
```

---

## 16. Warning Policy

Warnings are not stored in `profile.sqlite`. They are written to stderr and should not stop execution when a usable artifact can still be produced.

Examples:

```text
W_REFS_SOURCE_MISMATCH
W_CANONICAL_PATH_UNAVAILABLE
W_VALUE_TEXT_TRUNCATED
W_OBJECT_SAMPLE_LIMIT_REACHED
W_HEAVY_HITTER_LIMIT_REACHED
```

---

## 17. Performance Requirements

```text
- streaming source scan
- no full source materialization
- bounded exact counters
- bounded heavy hitter state
- bounded HLL state
- chunk-flushed object/value samples
- deterministic priority sampling for chunk-mergeable samples
- deferred canonical JSON/value text materialization
- batched SQLite writes
- bounded sample_json, value_json, parent_object_json, and value_text storage
```

`--perf-log` should emit timing buckets to stderr.

---

## 18. v0.1.0 Summary

v0.1.0 includes:

```text
- JSON input file
- JSONL input file
- no stdin
- refs/schemas.sqlite consumption
- profile.sqlite output
- default --refs refs/schemas.sqlite
- default --out profile.sqlite
- --quiet
- --perf-log
- --perf-log-file
- --perf-log-dbstat
- stdout summary from prof_source_summary + output path + elapsed time
- stderr warnings
- no --strict
- no separate report file
- no run/source/manifest/algorithm/warning tables
- no SQLite views
- shape-aware value profiling
- prof_object_sample for canonical/site/field_set/type_set samples
- first_seen and first_non_empty samples
- deterministic priority samples with chunk flush
- HyperLogLog distinct count
- Space-Saving heavy hitter candidates
- heavy_hitter_context disabled by default
- safer value context defaults for large JSONL
- bounded exact value distribution for small field profiles
```
