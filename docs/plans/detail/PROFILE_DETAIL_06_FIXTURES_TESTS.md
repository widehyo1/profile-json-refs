# Profile Detail 06: Fixtures and Regression Tests

Covers:

```text
Phase 11: fixtures and regression tests
```

---

## 1. Target Files

```text
tests/common/mod.rs
tests/cli_contract.rs
tests/sqlite_schema.rs
tests/refs_contract.rs
tests/json_scan.rs
tests/jsonl_scan.rs
tests/shape_identity.rs
tests/object_samples.rs
tests/heterogeneous_array.rs
tests/field_summary.rs
tests/exact_distribution.rs
tests/sketches.rs
tests/value_samples.rs
tests/sqlite_writer.rs
tests/output_contract.rs
fixtures/json/
fixtures/jsonl/
fixtures/refs/
fixtures/config/
scripts/assert_profile_sqlite.sh
scripts/make_fixture_refs.sh
```

---

## 2. Test Helper

`tests/common/mod.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TestRun {
    pub stdout: String,
    pub stderr: String,
    pub status: std::process::ExitStatus,
    pub out_sqlite: PathBuf,
}

pub fn run_profile(input: &Path, refs: &Path, out: &Path, args: &[&str]) -> TestRun {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_profile-json-refs"));
    cmd.arg(input)
        .arg("--refs").arg(refs)
        .arg("--out").arg(out);

    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().expect("command runs");

    TestRun {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        status: output.status,
        out_sqlite: out.to_path_buf(),
    }
}
```

SQLite helper:

```rust
pub fn sqlite_scalar_i64(db: &Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}
```

---

## 3. Fixture Matrix

### 3.1 Basic JSON

`fixtures/json/basic.json`:

```json
{
  "documents": [
    {"id": 1, "status": "active", "amount": 100},
    {"id": 2, "status": "inactive", "amount": 200}
  ]
}
```

Assertions:

```text
- profile.sqlite is written
- prof_source_summary has one row
- prof_shape has rows
- prof_field_summary has status and amount profiles
```

### 3.2 Basic JSONL

`fixtures/jsonl/basic.jsonl`:

```jsonl
{"id":1,"status":"active"}
{"id":2,"status":"inactive"}
{"id":3,"status":"active"}
```

Assertions:

```text
- document count = 3
- --jsonl works
```

### 3.3 Heterogeneous Shape

```json
[
  {"id": 1, "amount": 100},
  {"id": 2, "amount": "200"},
  {"id": 3, "error": "invalid"}
]
```

Assertions:

```text
- same canonical/site can have multiple prof_shape rows
- same field name amount with integer/string has separate field_profile_id
```

### 3.4 Heterogeneous Object Array

```json
{
  "items": [
    {"id": 1, "type": "A", "amount": 100},
    {"id": 2, "type": "B", "error": "invalid"},
    {"id": 3, "type": "A", "amount": "200"}
  ]
}
```

Assertions:

```sql
SELECT COUNT(*) >= 3
FROM prof_shape
WHERE site_path LIKE '%items%';
```

Also assert:

```text
- no prof_array_* tables
- no scalar array-specific tables
- object elements inside arrays are normal shape events
```

### 3.5 Empty first_seen then Non-Empty

```jsonl
{"payload":{}}
{"payload":{"id":1,"name":"A"}}
```

Assertions:

```text
- first_seen sample for payload is {}
- first_non_empty sample exists for payload
- first_non_empty sample is not {}
```

### 3.6 Empty String

```jsonl
{"code":null}
{"code":""}
{"code":"A"}
```

Assertions:

```text
- "" is first_non_empty if it is the first non-null/non-empty-structure value
- empty_string_count = 1
- observed_type string exists
```

### 3.7 Exact Distribution

```jsonl
{"status":"A"}
{"status":"B"}
{"status":"A"}
{"status":"C"}
```

Assertions:

```text
- distinct_count_method = exact
- prof_field_value rows use exact_full
- is_complete_distribution = 1
- counts are exact
```

### 3.8 Approximate Fallback

Generate distinct values above threshold:

```jsonl
{"id":"v000001"}
{"id":"v000002"}
...
```

Use test config:

```yaml
sampling:
  value:
    value_json_limit_bytes: 256
    parent_object_json_limit_bytes: 512
    priority_sample_limit_per_field_profile: 2
    heavy_hitter_context_sample_limit: 0

value_profile:
  exact_distinct_threshold: 16
  exact_value_bytes_per_field_profile: 4096
  global_exact_value_bytes_budget: 65536
  hll_precision: 10
  heavy_hitter_limit: 8
```

Assertions:

```text
- distinct_count_method = approximate
- distinct_algorithm = hyperloglog
- is_complete_distribution = 0
- heavy hitter rows <= 8
- heavy_hitter_context rows = 0 by default
```

### 3.9 Sample OOM Guard

Generate many sample keys with small priority limits.

Assertions:

```text
- priority sample rows per key <= configured limit
- first_seen row exists per materialized key
- command completes
- prof_field_value_sample does not grow proportional to high-cardinality heavy hitter candidates
```

---

## 4. Forbidden Schema Assertions

Every integration test that writes `profile.sqlite` should be able to call:

```sql
SELECT COUNT(*)
FROM sqlite_master
WHERE type = 'view';
```

Expected:

```text
0
```

Forbidden tables:

```sql
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND (
    name IN (
      'prof_path_sample',
      'prof_shape_sample',
      'prof_run',
      'prof_manifest',
      'prof_algorithm',
      'prof_warning'
    )
    OR name LIKE 'prof_array_%'
  );
```

Expected:

```text
no rows
```

Approved table set:

```sql
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND name LIKE 'prof_%'
ORDER BY name;
```

Expected:

```text
prof_field_summary
prof_field_value
prof_field_value_sample
prof_object_sample
prof_shape
prof_shape_field
prof_source_summary
```

---

## 5. Object Sample Coverage Assertions

For each sample scope:

```sql
SELECT sample_scope, sample_kind, COUNT(*)
FROM prof_object_sample
GROUP BY sample_scope, sample_kind
ORDER BY sample_scope, sample_kind;
```

Expected sample scopes:

```text
canonical_path
site_path
field_set
type_set
```

Required:

```text
- first_seen exists for every materialized sample key
- first_non_empty exists when fixture has non-empty candidate
- priority_sample count per key <= configured limit
```

Priority bound query:

```sql
SELECT sample_scope, sample_key, COUNT(*) AS c
FROM prof_object_sample
WHERE sample_kind = 'priority_sample'
GROUP BY sample_scope, sample_key
HAVING c > :limit;
```

Expected:

```text
no rows
```

Run per scope with the scope-specific limit.

---

## 6. Field Summary Assertions

Null-only:

```sql
SELECT profiled_count, null_count, non_null_count
FROM prof_field_summary
WHERE field_profile_id = :field_profile_id;
```

Expected:

```text
profiled_count = null_count
non_null_count = 0
```

Empty object only:

```text
profiled_count = empty_object_count
observed_type = object
```

Empty array only:

```text
profiled_count = empty_array_count
observed_type = array
```

Empty string:

```text
empty_string_count > 0
observed_type = string
non_null_count includes empty string
```

---

## 7. Value Distribution Assertions

Exact full distribution:

```sql
SELECT COUNT(*)
FROM prof_field_value
WHERE field_profile_id = :field_profile_id
  AND value_source = 'exact_full'
  AND count_method = 'exact'
  AND is_complete_distribution = 1;
```

Approximate fallback:

```sql
SELECT distinct_count_method, distinct_algorithm
FROM prof_field_summary
WHERE field_profile_id = :field_profile_id;
```

Expected:

```text
approximate, hyperloglog
```

Heavy hitter bound:

```sql
SELECT COUNT(*)
FROM prof_field_value
WHERE field_profile_id = :field_profile_id
  AND value_source = 'heavy_hitter';
```

Expected:

```text
<= heavy_hitter_limit
```

---

## 8. Output Contract Tests

Default stdout should match shape:

```text
profile-json-refs: wrote <path>

documents: <n>
objects: <n>
arrays: <n>
scalars: <n>
canonical_paths: <n>
site_paths: <n>
shapes: <n>
field_profiles: <n>
stored_values: <n>
elapsed: <seconds>s
```

Assertions:

```text
- stdout contains no warning code
- stdout contains no field row detail
- stdout contains no JSON report
- stderr contains warnings only when applicable
```

`--quiet`:

```text
stdout == ""
```

`--perf-log`:

```text
stderr contains incremental [perf] events
stdout remains summary-only unless --quiet
```

`--perf-log-file`:

```text
perf log file exists
perf log file is flushed during execution
```

`--perf-log-dbstat`:

```text
dbstat diagnostics appear only when explicitly enabled
```

---

## 9. Regression Script

`scripts/assert_profile_sqlite.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

db="${1:?usage: $0 profile.sqlite}"

sqlite3 "$db" <<'SQL'
.headers off
.mode list

SELECT 'views=' || COUNT(*) FROM sqlite_master WHERE type = 'view';

SELECT 'forbidden_tables=' || COUNT(*)
FROM sqlite_master
WHERE type = 'table'
  AND (
    name IN (
      'prof_path_sample',
      'prof_shape_sample',
      'prof_run',
      'prof_manifest',
      'prof_algorithm',
      'prof_warning'
    )
    OR name LIKE 'prof_array_%'
  );

SELECT 'summary_rows=' || COUNT(*) FROM prof_source_summary;
SQL
```

Use shell assertions in CI wrapper or Rust integration tests.

---

## 10. Performance Smoke Fixture

Create a generator script rather than checking in a large file.

`scripts/make_large_jsonl_fixture.py`:

```python
import json
import sys

n = int(sys.argv[1])
for i in range(n):
    row = {
        "id": f"id-{i}",
        "status": ["A", "B", "C"][i % 3],
        "payload": {} if i == 0 else {"x": i, "flag": i % 2 == 0},
        "items": [
            {"kind": "amount", "value": i},
            {"kind": "code", "value": str(i % 10)},
        ],
    }
    print(json.dumps(row, separators=(",", ":")))
```

Smoke assertions:

```text
- command exits 0
- profile.sqlite exists
- --perf-log emits progress events and final buckets
- heavy_hitter_context rows are 0 by default
- priority samples are bounded
- exact fallback occurs for high-cardinality id
```

No hard wall-clock pass/fail by default.

---

## 10.1 Script-Backed rc.2 Regression

Add a Rust integration test that runs a fixture, then invokes the read-only diagnosis script:

```bash
scripts/diagnose_profile_sqlite.sh \
  --fail-on-risk \
  --hh-context-limit 0 \
  --value-sample-limit 4 \
  <profile.sqlite>
```

Test name:

```text
rc2_diagnose_script_enforces_performance_safe_sample_contract
```

Expected TDD state:

```text
- before the rc.2 implementation, the test fails because current defaults still emit heavy_hitter_context rows
- after the rc.2 implementation, the test passes in cargo test
```

Also require the diagnostic and external regression scripts to be checked in and shell-syntax validated:

```text
scripts/diagnose_profile_sqlite.sh
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

The full external script requires a real `dump-json-refs` binary:

```bash
PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
DUMP_JSON_REFS_BIN=dump-json-refs \
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

It is a manual/perf CI gate rather than a default unit test because it depends on the upstream binary and creates refs from a generated JSONL fixture.

---

## 11. Phase 11 Commit

```bash
git add tests fixtures scripts
git commit -m "test(profile): add regression fixtures"
```
