# Profile Detail 05: SQLite Writer, Output, Warnings, and Performance

Covers:

```text
Phase 9: SQLite writer integration
Phase 10: stdout, stderr warnings, and perf-log
Phase 12: performance validation
```

---

## 1. Target Files

```text
src/sqlite/writer.rs
src/sqlite/schema.rs
src/perf/mod.rs
src/perf/timer.rs
src/lib.rs
src/main.rs
tests/sqlite_writer.rs
tests/output_contract.rs
tests/perf_log.rs
tests/perf_smoke.rs
```

---

## 2. Writer Structure

`src/sqlite/writer.rs`:

```rust
use rusqlite::{Connection, Transaction};

pub struct ProfileWriter {
    conn: Connection,
    object_sample_priority_limits: ObjectSamplePriorityLimits,
    value_priority_limit: usize,
}

pub struct ObjectSamplePriorityLimits {
    pub canonical_path: usize,
    pub site_path: usize,
    pub field_set: usize,
    pub type_set: usize,
}

impl ProfileWriter {
    pub fn open(path: &std::path::Path, config: &crate::config::ProfileConfig) -> crate::error::Result<Self> {
        let conn = Connection::open(path)?;
        crate::sqlite::schema::configure_connection(&conn)?;
        crate::sqlite::schema::create_schema(&conn)?;

        Ok(Self {
            conn,
            object_sample_priority_limits: ObjectSamplePriorityLimits {
                canonical_path: config.sampling.canonical_priority_limit,
                site_path: config.sampling.site_priority_limit,
                field_set: config.sampling.field_set_priority_limit,
                type_set: config.sampling.type_set_priority_limit,
            },
            value_priority_limit: config.sampling.value_priority_limit_per_field_profile,
        })
    }

    pub fn flush_chunk(&mut self, chunk: ProfileChunk) -> crate::error::Result<()> {
        let tx = self.conn.transaction()?;
        self.write_shapes(&tx, &chunk.shapes)?;
        self.write_shape_fields(&tx, &chunk.shape_fields)?;
        self.write_object_samples(&tx, &chunk.object_samples)?;
        self.write_field_summaries(&tx, &chunk.field_summaries)?;
        self.write_field_values(&tx, &chunk.field_values)?;
        self.write_value_samples(&tx, &chunk.value_samples)?;
        tx.commit()?;

        self.prune_object_priority_samples()?;
        self.prune_value_priority_samples()?;

        Ok(())
    }
}
```

`ProfileChunk` should contain row structs, not accumulator internals.

---

## 3. Row Structs

```rust
pub struct ProfileChunk {
    pub shapes: Vec<ShapeRow>,
    pub shape_fields: Vec<ShapeFieldRow>,
    pub object_samples: Vec<ObjectSampleRow>,
    pub field_summaries: Vec<FieldSummaryRow>,
    pub field_values: Vec<FieldValueRow>,
    pub value_samples: Vec<FieldValueSampleRow>,
}

impl ProfileChunk {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
            && self.shape_fields.is_empty()
            && self.object_samples.is_empty()
            && self.field_summaries.is_empty()
            && self.field_values.is_empty()
            && self.value_samples.is_empty()
    }
}
```

Keep row structs in `src/sqlite/writer.rs` or a `src/sqlite/rows.rs` module if they grow.

---

## 4. Upsert Rules

Shapes:

```sql
INSERT INTO prof_shape (
    shape_id,
    canonical_path,
    site_path,
    schema_path,
    field_set_hash,
    type_set_hash,
    field_set_json,
    type_set_json,
    object_count,
    first_seen_document_index,
    first_seen_path
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(shape_id) DO UPDATE SET
    object_count = object_count + excluded.object_count;
```

Shape fields:

```sql
INSERT INTO prof_shape_field (
    field_profile_id,
    shape_id,
    field_name,
    observed_type,
    observed_count,
    null_count
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
ON CONFLICT(field_profile_id) DO UPDATE SET
    observed_count = observed_count + excluded.observed_count,
    null_count = null_count + excluded.null_count;
```

Field summaries:

```sql
INSERT INTO prof_field_summary (
    field_profile_id,
    profiled_count,
    null_count,
    non_null_count,
    empty_object_count,
    empty_array_count,
    empty_string_count,
    distinct_count,
    distinct_count_method,
    distinct_algorithm,
    distinct_error_rate,
    stored_value_count
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(field_profile_id) DO UPDATE SET
    profiled_count = excluded.profiled_count,
    null_count = excluded.null_count,
    non_null_count = excluded.non_null_count,
    empty_object_count = excluded.empty_object_count,
    empty_array_count = excluded.empty_array_count,
    empty_string_count = excluded.empty_string_count,
    distinct_count = excluded.distinct_count,
    distinct_count_method = excluded.distinct_count_method,
    distinct_algorithm = excluded.distinct_algorithm,
    distinct_error_rate = excluded.distinct_error_rate,
    stored_value_count = excluded.stored_value_count;
```

For summaries, prefer writing final rows once at the end. If writing per chunk, preserve full accumulator state until final.

---

## 5. Object Sample Writes

First/non-empty:

```sql
INSERT OR IGNORE INTO prof_object_sample (
    object_sample_id,
    sample_scope,
    sample_key,
    canonical_path,
    site_path,
    schema_path,
    field_set_hash,
    type_set_hash,
    shape_id,
    sample_kind,
    document_index,
    source_path,
    sample_json,
    sample_json_truncated,
    sample_is_empty_object,
    sample_is_empty_array,
    sample_priority,
    sample_rank
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18);
```

Priority samples use normal insert. `object_sample_id` should include priority and source identity to avoid collision.

---

## 6. Object Priority Prune SQL

After each chunk flush:

```sql
DELETE FROM prof_object_sample
WHERE sample_kind = 'priority_sample'
  AND object_sample_id IN (
    SELECT object_sample_id
    FROM (
      SELECT
        object_sample_id,
        ROW_NUMBER() OVER (
          PARTITION BY sample_scope, sample_key
          ORDER BY sample_priority ASC, document_index ASC, source_path ASC
        ) AS rn
      FROM prof_object_sample
      WHERE sample_kind = 'priority_sample'
        AND sample_scope = ?1
    )
    WHERE rn > ?2
  );
```

Run once per scope:

```text
canonical_path -> canonical_priority_limit
site_path      -> site_priority_limit
field_set      -> field_set_priority_limit
type_set       -> type_set_priority_limit
```

Then update rank if desired:

```sql
WITH ranked AS (
  SELECT
    object_sample_id,
    ROW_NUMBER() OVER (
      PARTITION BY sample_scope, sample_key
      ORDER BY sample_priority ASC, document_index ASC, source_path ASC
    ) AS rn
  FROM prof_object_sample
  WHERE sample_kind = 'priority_sample'
)
UPDATE prof_object_sample
SET sample_rank = (
  SELECT rn FROM ranked WHERE ranked.object_sample_id = prof_object_sample.object_sample_id
)
WHERE object_sample_id IN (SELECT object_sample_id FROM ranked);
```

---

## 7. Value Sample Prune SQL

```sql
DELETE FROM prof_field_value_sample
WHERE sample_kind = 'priority_sample'
  AND value_sample_id IN (
    SELECT value_sample_id
    FROM (
      SELECT
        value_sample_id,
        ROW_NUMBER() OVER (
          PARTITION BY field_profile_id
          ORDER BY sample_priority ASC, document_index ASC, source_path ASC
        ) AS rn
      FROM prof_field_value_sample
      WHERE sample_kind = 'priority_sample'
    )
    WHERE rn > ?1
  );
```

`heavy_hitter_context` is disabled by default in `v0.1.0-rc.2`.

When disabled, no heavy hitter context prune should run.

If enabled later, heavy hitter context samples are finalization-only and bounded separately per final surviving `(field_profile_id, value_hash)`. They must not be generated or pruned for transient scan-time Space-Saving candidates.

---

## 8. Chunk Flush Triggers

Flush when any of these thresholds is reached:

```text
object sample pending rows >= chunk_object_sample_rows
value sample pending rows >= chunk_value_sample_rows
shape rows >= chunk_shape_rows
shape field rows >= chunk_field_rows
explicit end of source
```

At flush:

```text
1. convert chunk-local accumulator rows to row structs
2. write rows in one SQLite transaction
3. merge/prune priority samples
4. clear chunk-local sample state
5. keep global field value sketch state until final flush
```

Do not clear HLL, Space-Saving, or exact counters until finalization for the relevant field profile.

---

## 9. Source Summary Finalization

At finalization, compute counts from accumulators or SQLite tables.

Preferred robust method:

```sql
SELECT COUNT(*) FROM prof_shape;
SELECT COUNT(*) FROM prof_shape_field;
SELECT COUNT(*) FROM prof_field_value;
SELECT COUNT(DISTINCT canonical_path) FROM prof_shape;
SELECT COUNT(DISTINCT COALESCE(site_path, '')) FROM prof_shape;
```

Then insert one row into `prof_source_summary`.

```sql
DELETE FROM prof_source_summary;

INSERT INTO prof_source_summary (
    source_format,
    total_document_count,
    total_object_count,
    total_array_count,
    total_scalar_count,
    total_canonical_path_count,
    total_site_path_count,
    total_shape_count,
    total_field_profile_count,
    total_stored_value_count
)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);
```

---

## 10. Warning Model

Warnings are not stored in SQLite.

`src/lib.rs`:

```rust
#[derive(Debug, Clone)]
pub struct ProfileWarning {
    pub code: &'static str,
    pub message: String,
}
```

Warning codes:

```text
W_REFS_PRESENCE_SHAPES_TRUNCATED
W_CANONICAL_PATH_UNAVAILABLE
W_VALUE_TEXT_TRUNCATED
W_OBJECT_SAMPLE_TRUNCATED
W_HEAVY_HITTER_LIMIT_REACHED
W_EXACT_COUNTER_DISABLED
```

Emit duplicate warnings sparingly.

---

## 11. Stdout Summary

Default stdout:

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

No detailed rows, no JSON report, no warnings on stdout.

`--quiet`:

```text
stdout empty on success
stderr warnings/errors only
```

---

## 12. Perf Timer

`v0.1.0-rc.2` requires incremental perf logging.

`src/perf/timer.rs` should support:

```rust
use std::io::Write;
use std::time::{Duration, Instant};

pub enum PerfSink {
    Disabled,
    Stderr,
    File(std::fs::File),
}

pub struct PerfLog {
    start: Instant,
    sink: PerfSink,
    dbstat: bool,
    buckets: Vec<PerfBucket>,
}

pub struct PerfBucket {
    pub name: &'static str,
    pub duration: Duration,
}

impl PerfLog {
    pub fn disabled() -> Self {
        Self {
            start: Instant::now(),
            sink: PerfSink::Disabled,
            dbstat: false,
            buckets: Vec::new(),
        }
    }

    pub fn event(&mut self, phase: &str, fields: &[(&str, String)]) {
        if matches!(self.sink, PerfSink::Disabled) {
            return;
        }

        let mut line = format!("[perf] t={:.3}s phase={}", self.start.elapsed().as_secs_f64(), phase);
        for (k, v) in fields {
            line.push(' ');
            line.push_str(k);
            line.push('=');
            line.push_str(v);
        }
        self.write_line(&line);
    }

    pub fn write_line(&mut self, line: &str) {
        match &mut self.sink {
            PerfSink::Disabled => {}
            PerfSink::Stderr => {
                eprintln!("{line}");
            }
            PerfSink::File(file) => {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
    }
}
```

CLI behavior:

```text
--perf-log:
  write progress and final buckets to stderr

--perf-log-file <FILE>:
  write progress and final buckets to FILE and flush during execution

--perf-log-dbstat:
  include optional SQLite dbstat diagnostics
```

Required progress events:

```text
scan.progress:
  documents, objects, arrays, scalars, source position when available

flush.chunk:
  chunk index, shape rows, field rows, object sample rows, value sample rows

sqlite.prune_samples:
  prune kind, touched keys, rows before/after when practical, elapsed

sqlite.rows:
  row counts for key prof_* tables

sqlite.size:
  profile.sqlite / wal / shm bytes
```

Required final buckets:

```text
total
refs.open
refs.load_contract
sqlite.create_schema
scan.read_parse_walk
scan.observe_shapes
scan.observe_fields
scan.observe_values
flush.chunks.total
flush.object_samples
flush.value_samples
flush.shapes
flush.fields
flush.values
sqlite.prune_object_samples
sqlite.prune_value_samples
sqlite.write_field_summaries
sqlite.write_field_values
sqlite.write_source_summary
sqlite.indexes
stdout.summary
```


---

## 13. Performance Invariants

```text
- no full source materialization
- no unbounded per-key sample state
- sample JSON bodies are byte-limited
- value text is byte-limited
- exact counters have per-field and global budgets
- HLL memory is bounded by precision
- Space-Saving memory is bounded by heavy_hitter_limit
- heavy_hitter_context rows are 0 by default
- heavy_hitter_context, if enabled, is finalization-only
- priority samples are chunk-local plus SQLite-pruned
- SQLite writes are batched
```

Heterogeneous object arrays can increase shape and sample cardinality. v0.1.0 handles this through the same shape and sample bounds; it does not create array-specific accumulators.

---

## 14. Phase 9 Tests

```text
tests/sqlite_writer.rs
  - flush writes all approved tables
  - first_seen uses INSERT OR IGNORE
  - priority samples are pruned after flush
  - summary counts match table counts
```

---

## 15. Phase 10 Tests

```text
tests/output_contract.rs
  - default stdout contains output path and summary only
  - --quiet produces no stdout on success
  - warnings are stderr-only
  - errors are stderr and non-zero

tests/perf_log.rs
  - --perf-log emits incremental [perf] lines to stderr
  - --perf-log-file writes and flushes perf events to a file
  - --perf-log-dbstat is opt-in
  - --perf-log does not affect stdout
```

---

## 16. Phase 12 Tests

```text
tests/perf_smoke.rs
  - large finite JSONL fixture completes
  - profile.sqlite size remains bounded under configured limits
  - priority sample rows per key stay under limit
  - no prof_array_* table exists
```

Do not use wall-clock assertions in ordinary CI unless the environment is controlled. Prefer smoke checks and optional local benchmark scripts.

---

## 17. Commits

Phase 9:

```bash
git add src/sqlite src/lib.rs tests/sqlite_writer.rs
git commit -m "feat(sqlite): write profile facts in batches"
```

Phase 10:

```bash
git add src/main.rs src/perf tests/output_contract.rs tests/perf_log.rs
git commit -m "feat(cli): finalize output and perf log contract"
```

Phase 12:

```bash
git add tests/perf_smoke.rs scripts
git commit -m "perf(profile): validate large input profiling"
```
