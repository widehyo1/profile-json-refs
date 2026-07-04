# Profile Regression and Performance Plan

This document defines the `v0.1.0-rc.2` validation plan for `profile-json-refs`. rc.2 is expected to preserve the v0.1 SQLite/CLI contract while adding safer large-input defaults and strengthened perf diagnostics.

The goal is to verify that `profile-json-refs` writes a usable one-shot `profile.sqlite` fact artifact from a JSON/JSONL source file and `refs/schemas.sqlite`.

---

## 1. Validation Principles

### 1.1 Do Not Use Binary SQLite Goldens

Do not store `profile.sqlite` binary files as golden outputs.

Use SQL assertions instead.

### 1.2 One-Shot Artifact Contract

Every successful fixture run should produce one `profile.sqlite`.

The output DB must not contain:

```text
prof_run
prof_manifest
prof_algorithm
prof_warning
prof_path_sample
prof_shape_sample
prof_array_*
SQLite views
```

### 1.3 Warnings Are Not Fatal

Recoverable warnings are written to stderr.

A usable partial output should exit `0`.

### 1.4 stdout Is Summary Only

Default stdout must include only:

```text
output path
prof_source_summary-level summary
elapsed time
```

---

## 2. Fixture Layout

```text
fixtures/
├── basic_json/
│   ├── input.json
│   ├── refs.sqlite
│   └── assertions.sql
│
├── jsonl_basic/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
├── heterogeneous_shape/
│   ├── input.json
│   ├── refs.sqlite
│   └── assertions.sql
│
├── heterogeneous_object_array/
│   ├── input.json
│   ├── refs.sqlite
│   └── assertions.sql
│
├── empty_first_seen_then_non_empty/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
├── empty_string_value/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
├── exact_distribution/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
├── approximate_fallback/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
├── refs_truncated_presence_shape/
│   ├── input.jsonl
│   ├── refs.sqlite
│   └── assertions.sql
│
└── large_finite_jsonl_smoke/
    ├── input.jsonl
    ├── refs.sqlite
    └── assertions.sql
```

The `refs.sqlite` fixture should be produced by the approved `dump-json-refs` version or by a minimal fixture builder that matches the documented refs contract.

---

## 3. Universal SQL Assertions

### 3.1 Required Tables

```sql
SELECT name
FROM sqlite_master
WHERE type = 'table'
ORDER BY name;
```

Expected table set:

```text
prof_field_summary
prof_field_value
prof_field_value_sample
prof_object_sample
prof_shape
prof_shape_field
prof_source_summary
```

### 3.2 Forbidden Tables

```sql
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND (
    name IN (
      'prof_run',
      'prof_manifest',
      'prof_algorithm',
      'prof_warning',
      'prof_path_sample',
      'prof_shape_sample'
    )
    OR name LIKE 'prof_array_%'
  );
```

Expected row count:

```text
0
```

### 3.3 No Views

```sql
SELECT name
FROM sqlite_master
WHERE type = 'view';
```

Expected row count:

```text
0
```

### 3.4 Single Summary Row

```sql
SELECT COUNT(*) FROM prof_source_summary;
```

Expected:

```text
1
```

---

## 4. CLI Contract Tests

### 4.1 Defaults

```bash
profile-json-refs fixtures/basic_json/input.json
```

Expected:

```text
uses refs/schemas.sqlite unless overridden
writes profile.sqlite unless overridden
prints summary stdout
exit 0
```

### 4.2 Explicit Refs and Output

```bash
profile-json-refs fixtures/basic_json/input.json \
  --refs fixtures/basic_json/refs.sqlite \
  --out target/tmp/basic_profile.sqlite
```

Expected:

```text
target/tmp/basic_profile.sqlite exists
stdout contains "wrote target/tmp/basic_profile.sqlite"
```

### 4.3 JSONL Mode

```bash
profile-json-refs fixtures/jsonl_basic/input.jsonl \
  --jsonl \
  --refs fixtures/jsonl_basic/refs.sqlite \
  --out target/tmp/jsonl_profile.sqlite
```

Expected:

```text
prof_source_summary.source_format = jsonl
```

### 4.4 Stdin Rejection

Unsupported:

```bash
cat fixtures/jsonl_basic/input.jsonl | profile-json-refs --jsonl
profile-json-refs - --jsonl
```

Expected:

```text
non-zero exit
stderr explains stdin is unsupported in v0.1.0
no usable profile.sqlite created
```

### 4.5 No Strict

```bash
profile-json-refs fixtures/basic_json/input.json --strict
```

Expected:

```text
non-zero exit
stderr indicates unknown option
```

---

## 5. stdout / stderr Assertions

### 5.1 Default stdout

Default stdout may include only:

```text
profile-json-refs: wrote <output>

documents: <n>
objects: <n>
arrays: <n>
scalars: <n>
canonical_paths: <n>
site_paths: <n>
shapes: <n>
field_profiles: <n>
stored_values: <n>
elapsed: <duration>s
```

Assertions:

```text
stdout contains output path
stdout contains elapsed
stdout does not contain detailed prof_shape rows
stdout does not contain field names or value rows
```

### 5.2 Quiet

```bash
profile-json-refs fixtures/basic_json/input.json \
  --refs fixtures/basic_json/refs.sqlite \
  --out target/tmp/profile.sqlite \
  --quiet
```

Expected:

```text
stdout empty on success
stderr warnings/errors only
```

### 5.3 Perf Log

```bash
profile-json-refs fixtures/basic_json/input.json \
  --refs fixtures/basic_json/refs.sqlite \
  --out target/tmp/profile.sqlite \
  --perf-log
```

Expected stderr contains incremental perf events and final buckets:

```text
[perf] t=
[perf] ... phase=scan.progress
[perf] ... phase=flush.chunk
[perf] total=
[perf] refs.open=
[perf] scan.read_parse_walk=
[perf] sqlite.indexes=
```

Expected stdout remains summary-only.

### 5.4 Perf Log File and dbstat

```bash
profile-json-refs fixtures/basic_json/input.json \
  --refs fixtures/basic_json/refs.sqlite \
  --out target/tmp/profile.sqlite \
  --perf-log \
  --perf-log-file target/tmp/perf.log \
  --perf-log-dbstat
```

Expected:

```text
perf.log exists
perf.log contains [perf] events before process completion
stderr remains available for warnings/errors
dbstat diagnostics appear only when --perf-log-dbstat is set
```

---

## 6. Sampling Regression Tests

### 6.1 Object Sample Coverage

For every materialized `prof_shape`, the `type_set` grain should have a `first_seen` sample.

```sql
SELECT COUNT(*)
FROM prof_shape s
WHERE NOT EXISTS (
  SELECT 1
  FROM prof_object_sample os
  WHERE os.sample_scope = 'type_set'
    AND os.shape_id = s.shape_id
    AND os.sample_kind = 'first_seen'
);
```

Expected:

```text
0
```

### 6.2 Four-Grain Coverage

Assert first_seen samples exist for materialized grains:

```text
canonical_path
site_path
field_set
type_set
```

### 6.3 first_non_empty after Empty first_seen

Fixture pattern:

```json
{}
{"id": 1, "name": "A"}
```

Expected:

```text
first_seen sample exists and may be {}
first_non_empty sample exists and contains object fields
```

### 6.4 Empty String Is Non-Empty

Fixture pattern:

```json
{"name": ""}
{"name": "Alice"}
```

Expected:

```text
"" is eligible for first_non_empty value sample
empty_string_count is incremented
observed_type is string
```

SQL:

```sql
SELECT fs.empty_string_count
FROM prof_field_summary fs
JOIN prof_shape_field sf
  ON sf.field_profile_id = fs.field_profile_id
WHERE sf.field_name = 'name'
  AND sf.observed_type = 'string';
```

Expected:

```text
>= 1
```

### 6.5 Priority Sample Limit

For each sample key:

```sql
SELECT sample_scope, sample_key, COUNT(*)
FROM prof_object_sample
WHERE sample_kind = 'priority_sample'
GROUP BY sample_scope, sample_key
HAVING COUNT(*) > :configured_limit;
```

Expected:

```text
0
```

### 6.6 Heavy Hitter Context Disabled by Default

`v0.1.0-rc.2` disables heavy hitter context samples by default.

```sql
SELECT COUNT(*)
FROM prof_field_value_sample
WHERE sample_kind = 'heavy_hitter_context';
```

Expected:

```text
0
```

High-cardinality fields must not produce `heavy_hitter_context` rows proportional to distinct value count.

---

## 7. Heterogeneous Object Array Regression

Fixture:

```json
{
  "items": [
    {"id": 1, "type": "A", "amount": 100},
    {"id": 2, "type": "B", "error": "invalid"},
    {"id": 3, "type": "A", "amount": "200"}
  ]
}
```

Expected:

```text
one array site may produce multiple prof_shape rows
different field_set_hash or type_set_hash separates shapes
no prof_array_* tables exist
```

SQL:

```sql
SELECT COUNT(*)
FROM prof_shape
WHERE site_path LIKE '%items%';
```

Expected:

```text
>= 2
```

No array tables:

```sql
SELECT COUNT(*)
FROM sqlite_master
WHERE type = 'table'
  AND name LIKE 'prof_array_%';
```

Expected:

```text
0
```

---

## 8. Field Summary Regression

### 8.1 Null Only

Expected:

```text
profiled_count = null_count
non_null_count = 0
```

### 8.2 Empty Object Only

Expected:

```text
profiled_count = empty_object_count
observed_type = object
```

### 8.3 Empty Array Only

Expected:

```text
profiled_count = empty_array_count
observed_type = array
```

### 8.4 Empty String Only

Expected:

```text
profiled_count = empty_string_count
observed_type = string
non_null_count = profiled_count
```

---

## 9. Exact Distribution Regression

Fixture:

```jsonl
{"status":"A"}
{"status":"B"}
{"status":"A"}
{"status":"C"}
```

Expected:

```text
distinct_count_method = exact
prof_field_value.value_source = exact_full
prof_field_value.count_method = exact
is_complete_distribution = 1
```

---

## 10. Approximate Fallback Regression

Fixture should exceed one of:

```text
exact_distinct_threshold
exact_value_bytes_per_field_profile
global_exact_value_bytes_budget
```

Expected:

```text
distinct_count_method = approximate
distinct_algorithm = hyperloglog
heavy hitter rows are bounded
is_complete_distribution = 0
```

---

## 11. Refs Truncation Regression

Fixture uses refs DB where presence shape limits indicate truncation.

Expected:

```text
stderr warning emitted
profile generation continues
profile.sqlite exists
exit 0
```

SQL should not require missing upstream shape identities to exist.

---

## 12. Performance Smoke Checks

Performance tests are not exact benchmarks. They check structural performance properties.

### 12.1 Large Finite JSONL Smoke

Expected:

```text
run completes
profile.sqlite exists
--perf-log contains progress events and final buckets
heavy_hitter_context rows are 0 by default
priority samples remain bounded
exact counters fall back when configured threshold is low
```

### 12.2 Sample OOM Guard

Use a fixture with many sample keys.

Expected:

```text
no unbounded per-key sample state
chunk flush occurs
SQLite rows remain within configured priority sample limits
```

### 12.3 Deferred Materialization Guard

Use a fixture with repeated values and repeated shapes.

Expected:

```text
hot path does not require canonical JSON string materialization for every observed value
value_text is materialized only for stored values or samples
```

This may initially be verified by code review and perf-log timing, then by allocation profiling later.

---

## 13. Script-Backed Regression Harness

The rc.2 performance fix must be guarded by executable scripts as well as Rust integration tests.

### 13.1 Cargo-Level Diagnostic Regression

`tests/profile_fixtures.rs` contains:

```text
rc2_diagnose_script_enforces_performance_safe_sample_contract
```

This test runs `profile-json-refs` on a generated fixture, then runs:

```bash
scripts/diagnose_profile_sqlite.sh \
  --fail-on-risk \
  --hh-context-limit 0 \
  --value-sample-limit 4 \
  <profile.sqlite>
```

Before the rc.2 implementation lands, this test is expected to fail against the current rc.1 behavior because:

```text
heavy_hitter_context rows exist
value priority samples may exceed the rc.2 default limit
```

After the fix, this test must pass in the default `cargo test` suite.

### 13.2 Full External Regression Script

Use:

```bash
PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
DUMP_JSON_REFS_BIN=dump-json-refs \
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

This script is the full end-to-end rc.2 performance-patch regression harness:

```text
- approved prof_* table set is unchanged
- no SQLite views
- no forbidden run/manifest/warning/path/shape/array tables
- stdin and --strict remain unsupported
- stdout is summary-only
- --quiet suppresses stdout
- --perf-log emits [perf] lines
- heavy_hitter_context rows are 0 with the performance-safe config
- high-cardinality fields do not produce heavy_hitter_context rows
- prof_field_summary and prof_field_value are finalized
```

Optional baseline comparison:

```bash
PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
BASELINE_PROFILE_JSON_REFS_BIN=/path/to/v0.1.0/profile-json-refs \
DUMP_JSON_REFS_BIN=dump-json-refs \
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

When a baseline binary is provided, compare schema signatures only. Row counts are not golden outputs because the performance patch intentionally changes sample retention.

### 13.3 Read-Only Diagnosis Script

Use `scripts/diagnose_profile_sqlite.sh` for manual investigation and CI-style risk checks:

```bash
scripts/diagnose_profile_sqlite.sh --fail-on-risk profile.sqlite
```

For rc.2 defaults:

```bash
scripts/diagnose_profile_sqlite.sh \
  --fail-on-risk \
  --heavy-hitter-limit 128 \
  --hh-context-limit 0 \
  --value-sample-limit 4 \
  profile.sqlite
```

The script reports:

```text
- file sizes
- required/forbidden schema objects
- row counts
- SQLite dbstat table sizes when available
- value sample distribution and payload pressure
- heavy_hitter_context risk counts
- priority sample bound violations
- shape cardinality diagnostics
- risk summary
```

It is read-only and must not modify `profile.sqlite`.

### 13.4 Supporting Scripts

Existing fixture helpers remain useful:

```text
scripts/assert_profile_sqlite.sh
  Lightweight SQL assertion helper for fixture artifacts.

scripts/make_fixture_refs.sh
  Minimal refs fixture builder for small integration tests.

scripts/make_large_jsonl_fixture.py
  Large finite JSONL generator for smoke/performance checks.
```

---

## 14. CI Gate

Minimum CI gate:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Performance smoke may be optional in default CI if the fixture is too large.

Recommended split:

```text
default CI:
  unit + integration + small fixtures
  cargo-level diagnostic regression using diagnose_profile_sqlite.sh

manual/perf CI:
  scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
  large finite JSONL smoke + perf-log capture
  diagnose_profile_sqlite.sh --fail-on-risk on captured profile.sqlite
```

---

## 15. Definition of Done

v0.1.0 regression/performance validation is complete when:

```text
1. All small fixture tests pass.
2. CLI default output matches summary-only stdout.
3. --quiet suppresses stdout.
4. --perf-log writes timing buckets to stderr.
5. stdin and '-' are rejected.
6. No forbidden tables or views are created.
7. prof_source_summary has one row.
8. Object samples cover canonical/site/field_set/type_set grains.
9. first_seen is guaranteed for materialized sample keys.
10. first_non_empty is written when available.
11. "" is treated as non-empty string.
12. Null-only, empty-object-only, empty-array-only, and empty-string-only counters are verified.
13. Heterogeneous object arrays produce multiple prof_shape rows under one array site.
14. No prof_array_* tables are created.
15. Exact full distribution works for bounded small field profiles.
16. Approximate fallback works for large field profiles.
17. Priority samples are chunk-flushed and bounded.
18. Large finite JSONL smoke run completes.
19. `diagnose_profile_sqlite.sh --fail-on-risk` passes with rc.2 limits.
20. `regression_profile_json_refs_v0_1_rc2_patch.sh` passes against the candidate binary.
```
