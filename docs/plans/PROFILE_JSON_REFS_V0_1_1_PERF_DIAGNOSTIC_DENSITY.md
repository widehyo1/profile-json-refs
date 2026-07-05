# profile-json-refs v0.1.1 Perf Diagnostic Density Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Treat `v0.1.0-rc.2` as the shipped `v0.1.0` baseline and make `v0.1.1` improve performance-diagnosis density in existing `--perf-log` output without changing the CLI surface or stdout contract.

**Architecture:** Keep `profile-json-refs` as a downstream profile-fact layer over stable `dump-json-refs` artifacts. Add finer-grained timing and row-count diagnostics around profile scan accumulators, SQLite chunk writes, prune work, final summary writing, index creation, and optional dbstat output. Do not add options, YAML fields, stdout lines, SQLite tables, or upstream refs responsibilities.

**Tech Stack:** Rust 2024, clap existing CLI only, rusqlite bundled SQLite, cargo integration tests.

---

## Required Reading

- `/home/widehyo/gitclone/dump-json-refs/docs/references/REFS_REFERENCE_IMPLEMENTATION.md`
- `docs/PERFORMANCE.md`
- `docs/PROFILE_JSON_REFS_PERF_DIAGNOSIS.md`
- `docs/PROFILE_REGRESSION_AND_PERFORMANCE_PLAN.md`
- `docs/CLI_CONTRACT.md`
- `baselines/v0.1.0-rc.2/perf.log`
- `temp/perf.log`
- `temp/main.rs`

`temp/main.rs` is useful only as a diagnostic-output pattern reference. Do not port upstream refs stages or raw traversal responsibilities into this repository.

---

## Scope

### In Scope

- Enrich existing `--perf-log` stderr/file output.
- Keep current `--perf-log-file` flushing behavior.
- Keep current `--perf-log-dbstat` as the only optional expensive diagnostic gate.
- Add row counts, touched-key counts, elapsed milliseconds, and final SQLite size facts where they are cheap to collect.
- Add tests that assert diagnostic lines exist without depending on volatile timing values.

### Out Of Scope

- No new CLI options.
- No new YAML config keys.
- No stdout changes.
- No default-output changes when `--perf-log` is not set.
- No SQLite artifact schema changes.
- No refs traversal, refs schema, or upstream contract changes.
- No performance optimization in this plan except trivial instrumentation overhead avoidance.

---

## Diagnostic Contract For v0.1.1

`--perf-log` remains an explicitly requested diagnostic stream. v0.1.1 may add lines to this stream.

Required new event families:

```text
phase=scan.progress ...
phase=scan.accumulators ...
phase=flush.chunk ...
phase=sqlite.flush.shapes ...
phase=sqlite.flush.shape_fields ...
phase=sqlite.flush.object_samples ...
phase=sqlite.flush.field_summaries ...
phase=sqlite.flush.field_values ...
phase=sqlite.flush.value_samples ...
phase=sqlite.flush.commit ...
phase=sqlite.prune.object_priority ...
phase=sqlite.prune.value_priority ...
phase=sqlite.prune.heavy_hitter_context ...
phase=sqlite.indexes ...
phase=sqlite.summary.counts ...
phase=sqlite.summary.write ...
phase=sqlite.size ...
```

`--perf-log-dbstat` should continue to be opt-in and should emit more than one table when `dbstat` is available:

```text
phase=sqlite.dbstat table=<name> mb=<n> rank=<n>
phase=sqlite.dbstat unavailable=1
```

The exact elapsed values are non-contractual. Tests should check stable labels and stable count keys only.

---

## Files To Modify

```text
src/perf/timer.rs
  Add a small helper for elapsed-ms events so callers do not hand-format durations inconsistently.

src/lib.rs
  Add scan accumulator progress events, pass perf logging into writer flushes, and record final summary/index/size events.

src/sqlite/writer.rs
  Instrument each table write, transaction commit, prune family, summary count/write stages, index creation, dbstat top-N, and cheap file-size reporting support where appropriate.

tests/perf_log.rs
  Add integration tests for the new event families and for no stdout change.

docs/PERFORMANCE.md
  Update the v0.1.1 perf-log section after implementation.

docs/CLI_CONTRACT.md
  Do not change CLI syntax. Only update the performance-log example if needed to show additive diagnostic lines under the existing option.
```

Do not modify `src/cli.rs` or add config fields for this plan.

---

### Task 1: Add Failing Perf-Log Density Tests

**Files:**
- Modify: `tests/perf_log.rs`

- [x] **Step 1: Add an event assertion helper**

Add this helper below `REQUIRED_BUCKETS` in `tests/perf_log.rs`:

```rust
fn assert_perf_contains(stderr: &str, needle: &str) {
    assert!(
        stderr.contains(needle),
        "missing perf diagnostic {needle:?} in stderr:\n{stderr}"
    );
}
```

- [x] **Step 2: Add failing test for scan and writer diagnostic density**

Add this test to `tests/perf_log.rs`:

```rust
#[test]
fn perf_log_emits_scan_and_sqlite_detail_events() {
    let fixture = basic_fixture(
        "perf-log-density",
        r#"{"id":1,"name":"Ada","tags":["engineer"]}
{"id":2,"name":"Grace","tags":["compiler","navy"]}"#,
        true,
    );

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--jsonl".to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stderr = stderr(&output);

    assert_perf_contains(&stderr, "phase=scan.progress");
    assert_perf_contains(&stderr, "phase=scan.accumulators");
    assert_perf_contains(&stderr, "pending_shapes=");
    assert_perf_contains(&stderr, "pending_shape_fields=");
    assert_perf_contains(&stderr, "pending_value_samples=");

    assert_perf_contains(&stderr, "phase=flush.chunk");
    assert_perf_contains(&stderr, "field_summaries=");
    assert_perf_contains(&stderr, "field_values=");

    assert_perf_contains(&stderr, "phase=sqlite.flush.shapes");
    assert_perf_contains(&stderr, "phase=sqlite.flush.shape_fields");
    assert_perf_contains(&stderr, "phase=sqlite.flush.object_samples");
    assert_perf_contains(&stderr, "phase=sqlite.flush.field_summaries");
    assert_perf_contains(&stderr, "phase=sqlite.flush.field_values");
    assert_perf_contains(&stderr, "phase=sqlite.flush.value_samples");
    assert_perf_contains(&stderr, "phase=sqlite.flush.commit");
    assert_perf_contains(&stderr, "elapsed_ms=");
    assert_perf_contains(&stderr, "rows=");
}
```

- [x] **Step 3: Add failing test for final SQLite diagnostics**

Add this test to `tests/perf_log.rs`:

```rust
#[test]
fn perf_log_emits_final_sqlite_summary_and_size_events() {
    let fixture = basic_fixture("perf-log-final-density", r#"{"id":1,"name":"Ada"}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stderr = stderr(&output);

    assert_perf_contains(&stderr, "phase=sqlite.prune.object_priority");
    assert_perf_contains(&stderr, "phase=sqlite.prune.value_priority");
    assert_perf_contains(&stderr, "phase=sqlite.summary.counts");
    assert_perf_contains(&stderr, "phase=sqlite.summary.write");
    assert_perf_contains(&stderr, "phase=sqlite.size");
    assert_perf_contains(&stderr, "profile_sqlite_bytes=");
}
```

- [x] **Step 4: Run tests and verify they fail for missing diagnostics**

Run:

```bash
cargo test perf_log_emits -- --nocapture
```

Expected:

```text
FAIL
missing perf diagnostic "phase=scan.accumulators"
```

The exact first missing diagnostic may differ, but the failure must be caused by absent v0.1.1 perf diagnostics, not fixture setup or command failure.

---

### Task 2: Add Perf Event Helper

**Files:**
- Modify: `src/perf/timer.rs`

- [x] **Step 1: Add elapsed event helper**

In `impl PerfLog`, add:

```rust
    pub fn elapsed_event(&mut self, phase: &str, started: Instant, detail: impl std::fmt::Display) {
        if !self.enabled {
            return;
        }
        self.event(&format!(
            "phase={phase} elapsed_ms={} {detail}",
            started.elapsed().as_millis()
        ));
    }
```

- [x] **Step 2: Run focused tests and verify failures move forward**

Run:

```bash
cargo test perf_log_emits -- --nocapture
```

Expected:

```text
FAIL
missing perf diagnostic "phase=scan.accumulators"
```

The helper alone should not make the tests pass.

- [x] **Step 3: Commit**

```bash
git add src/perf/timer.rs tests/perf_log.rs
git commit -m "test(perf): require v0.1.1 diagnostic density"
```

---

### Task 3: Emit Scan Accumulator and Chunk Detail

**Files:**
- Modify: `src/lib.rs`

- [x] **Step 1: Add scan accumulator event method**

In `impl ProfileRunVisitor`, add:

```rust
    fn emit_scan_accumulators(&mut self) {
        self.perf_log.event(&format!(
            "phase=scan.accumulators pending_shapes={} pending_shape_fields={} pending_object_samples={} pending_value_samples={} field_value_accumulators={}",
            self.shape_accumulator.shape_row_count(),
            self.shape_fields.len(),
            self.shape_accumulator.pending_object_sample_count(),
            self.pending_value_sample_count(),
            self.field_values.len()
        ));
    }
```

- [x] **Step 2: Call accumulator event after scan progress**

In `run`, after:

```rust
    visitor.emit_scan_progress();
```

add:

```rust
    visitor.emit_scan_accumulators();
```

- [x] **Step 3: Enrich flush.chunk event**

Change `emit_flush_chunk` to include finalization rows:

```rust
    fn emit_flush_chunk(&mut self, chunk: &ProfileChunk) {
        self.perf_log.event(&format!(
            "phase=flush.chunk shapes={} fields={} object_samples={} field_summaries={} field_values={} value_samples={}",
            chunk.shapes.len(),
            chunk.shape_fields.len(),
            chunk.object_samples.len(),
            chunk.field_summaries.len(),
            chunk.field_values.len(),
            chunk.value_samples.len()
        ));
    }
```

- [x] **Step 4: Run focused tests**

Run:

```bash
cargo test perf_log_emits_scan_and_sqlite_detail_events -- --nocapture
```

Expected:

```text
FAIL
missing perf diagnostic "phase=sqlite.flush.shapes"
```

- [x] **Step 5: Commit**

```bash
git add src/lib.rs tests/perf_log.rs
git commit -m "feat(perf): log scan accumulator density"
```

---

### Task 4: Instrument SQLite Chunk Flush Internals

**Files:**
- Modify: `src/sqlite/writer.rs`
- Modify: `src/lib.rs`

- [x] **Step 1: Import PerfLog in writer**

At the top of `src/sqlite/writer.rs`, add:

```rust
use crate::perf::timer::PerfLog;
use std::time::Instant;
```

- [x] **Step 2: Change flush_chunk signature**

Change:

```rust
    pub fn flush_chunk(&mut self, chunk: ProfileChunk) -> Result<()> {
```

to:

```rust
    pub fn flush_chunk(&mut self, chunk: ProfileChunk, perf_log: &mut PerfLog) -> Result<()> {
```

- [x] **Step 3: Time each table write**

Inside `flush_chunk`, replace the six write calls with timed sections:

```rust
        let started = Instant::now();
        Self::write_shapes(&tx, &chunk.shapes)?;
        perf_log.elapsed_event(
            "sqlite.flush.shapes",
            started,
            format_args!("rows={}", chunk.shapes.len()),
        );

        let started = Instant::now();
        Self::write_shape_fields(&tx, &chunk.shape_fields)?;
        perf_log.elapsed_event(
            "sqlite.flush.shape_fields",
            started,
            format_args!("rows={}", chunk.shape_fields.len()),
        );

        let started = Instant::now();
        Self::write_object_samples(&tx, &chunk.object_samples)?;
        perf_log.elapsed_event(
            "sqlite.flush.object_samples",
            started,
            format_args!("rows={}", chunk.object_samples.len()),
        );

        let started = Instant::now();
        Self::write_field_summaries(&tx, &chunk.field_summaries)?;
        perf_log.elapsed_event(
            "sqlite.flush.field_summaries",
            started,
            format_args!("rows={}", chunk.field_summaries.len()),
        );

        let started = Instant::now();
        Self::write_field_values(&tx, &chunk.field_values)?;
        perf_log.elapsed_event(
            "sqlite.flush.field_values",
            started,
            format_args!("rows={}", chunk.field_values.len()),
        );

        let started = Instant::now();
        Self::write_value_samples(&tx, &chunk.value_samples)?;
        perf_log.elapsed_event(
            "sqlite.flush.value_samples",
            started,
            format_args!("rows={}", chunk.value_samples.len()),
        );

        let started = Instant::now();
        tx.commit()?;
        perf_log.elapsed_event("sqlite.flush.commit", started, format_args!("rows=0"));
```

- [x] **Step 4: Update lib.rs call site**

Change `ProfileRunVisitor::flush_chunk` from:

```rust
        self.writer.flush_chunk(chunk)
```

to:

```rust
        self.writer.flush_chunk(chunk, &mut self.perf_log)
```

- [x] **Step 5: Run focused density test**

Run:

```bash
cargo test perf_log_emits_scan_and_sqlite_detail_events -- --nocapture
```

Expected:

```text
PASS
```

- [x] **Step 6: Commit**

```bash
git add src/lib.rs src/sqlite/writer.rs tests/perf_log.rs
git commit -m "feat(perf): log sqlite flush stages"
```

---

### Task 5: Instrument Prune Families

**Files:**
- Modify: `src/sqlite/writer.rs`
- Modify: `src/lib.rs`

- [x] **Step 1: Add prune timing in writer flush_chunk**

Replace the prune block in `ProfileWriter::flush_chunk` with:

```rust
        let started = Instant::now();
        self.prune_object_priority_samples(&touched_samples.object_priority)?;
        perf_log.elapsed_event(
            "sqlite.prune.object_priority",
            started,
            format_args!("keys={}", touched_samples.object_priority.len()),
        );

        let started = Instant::now();
        self.prune_value_priority_samples(&touched_samples.value_priority_fields)?;
        perf_log.elapsed_event(
            "sqlite.prune.value_priority",
            started,
            format_args!("fields={}", touched_samples.value_priority_fields.len()),
        );

        if self.heavy_hitter_context_sample_limit > 0 {
            let started = Instant::now();
            self.prune_heavy_hitter_context_samples(&touched_samples.heavy_hitter_context)?;
            perf_log.elapsed_event(
                "sqlite.prune.heavy_hitter_context",
                started,
                format_args!("keys={}", touched_samples.heavy_hitter_context.len()),
            );
        } else {
            perf_log.event("phase=sqlite.prune.heavy_hitter_context skipped=1 keys=0");
        }
```

- [x] **Step 2: Remove zero-duration aggregate prune bucket**

In `ProfileRunVisitor::finish`, remove:

```rust
        self.perf_log.record("sqlite.prune_samples", Duration::ZERO);
```

Keep `sqlite.prune_samples` in tests only if backwards compatibility is intentionally required. For v0.1.1 diagnostic density, per-family events replace the zero-duration bucket.

- [x] **Step 3: Update `REQUIRED_BUCKETS` if needed**

If `tests/perf_log.rs` still requires `sqlite.prune_samples`, delete that entry from `REQUIRED_BUCKETS`. Do not remove any real timing bucket that still carries useful information.

- [x] **Step 4: Run final diagnostics test**

Run:

```bash
cargo test perf_log_emits_final_sqlite_summary_and_size_events -- --nocapture
```

Expected:

```text
FAIL
missing perf diagnostic "phase=sqlite.summary.counts"
```

- [x] **Step 5: Commit**

```bash
git add src/lib.rs src/sqlite/writer.rs tests/perf_log.rs
git commit -m "feat(perf): log sqlite prune families"
```

---

### Task 6: Instrument Summary, Index, and File Size Diagnostics

**Files:**
- Modify: `src/sqlite/writer.rs`
- Modify: `src/lib.rs`

- [x] **Step 1: Add timed summary writer**

Change `write_source_summary` signature in `src/sqlite/writer.rs` from:

```rust
    pub fn write_source_summary(
        &mut self,
        source_format: &str,
        counters: SourceCounters,
    ) -> Result<SourceSummary> {
```

to:

```rust
    pub fn write_source_summary(
        &mut self,
        source_format: &str,
        counters: SourceCounters,
        perf_log: &mut PerfLog,
    ) -> Result<SourceSummary> {
```

- [x] **Step 2: Time summary count queries**

At the start of `write_source_summary`, add:

```rust
        let counts_started = Instant::now();
```

After `total_stored_value_count` is computed, add:

```rust
        perf_log.elapsed_event(
            "sqlite.summary.counts",
            counts_started,
            format_args!(
                "canonical_paths={} site_paths={} shapes={} field_profiles={} stored_values={}",
                total_canonical_path_count,
                total_site_path_count,
                total_shape_count,
                total_field_profile_count,
                total_stored_value_count
            ),
        );
```

- [x] **Step 3: Time summary table write**

Before `let tx = self.conn.transaction()?;`, add:

```rust
        let write_started = Instant::now();
```

After `tx.commit()?;`, add:

```rust
        perf_log.elapsed_event("sqlite.summary.write", write_started, format_args!("rows=1"));
```

- [x] **Step 4: Update summary call site**

In `ProfileRunVisitor::finish`, change:

```rust
        let summary = self
            .writer
            .write_source_summary(source_format, self.counters)?;
```

to:

```rust
        let summary = self.writer.write_source_summary(
            source_format,
            self.counters,
            &mut self.perf_log,
        )?;
```

- [x] **Step 5: Add detailed index event**

In `ProfileRunVisitor::finish`, replace:

```rust
        self.perf_log
            .time_result("sqlite.indexes", || self.writer.create_indexes())?;
```

with:

```rust
        let index_start = Instant::now();
        self.writer.create_indexes()?;
        self.perf_log.record("sqlite.indexes", index_start.elapsed());
        self.perf_log.elapsed_event("sqlite.indexes", index_start, format_args!("created=1"));
```

- [x] **Step 6: Add SQLite file size event**

In `ProfileRunVisitor::finish`, after `write_source_summary`, add:

```rust
        self.emit_sqlite_size(&out_path);
```

Add this method to `impl ProfileRunVisitor`:

```rust
    fn emit_sqlite_size(&mut self, out_path: &std::path::Path) {
        let sqlite_bytes = std::fs::metadata(out_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let wal_bytes = std::fs::metadata(out_path.with_extension("sqlite-wal"))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let shm_bytes = std::fs::metadata(out_path.with_extension("sqlite-shm"))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        self.perf_log.event(&format!(
            "phase=sqlite.size profile_sqlite_bytes={} profile_sqlite_wal_bytes={} profile_sqlite_shm_bytes={}",
            sqlite_bytes, wal_bytes, shm_bytes
        ));
    }
```

- [x] **Step 7: Run final diagnostics test**

Run:

```bash
cargo test perf_log_emits_final_sqlite_summary_and_size_events -- --nocapture
```

Expected:

```text
PASS
```

- [x] **Step 8: Commit**

```bash
git add src/lib.rs src/sqlite/writer.rs tests/perf_log.rs
git commit -m "feat(perf): log sqlite finalization density"
```

---

### Task 7: Enrich dbstat Without New Options

**Files:**
- Modify: `src/sqlite/writer.rs`
- Modify: `src/lib.rs`
- Modify: `tests/perf_log.rs`

- [x] **Step 1: Add failing dbstat top-N assertion**

In `perf_log_dbstat_is_opt_in`, after:

```rust
    assert!(stderr(&with).contains("phase=sqlite.dbstat"));
```

add:

```rust
    assert!(stderr(&with).contains("rank=1"));
    assert!(stderr(&with).contains("table=") || stderr(&with).contains("unavailable=1"));
```

- [x] **Step 2: Replace single dbstat summary with rows**

In `src/sqlite/writer.rs`, replace `dbstat_summary` with:

```rust
    pub fn dbstat_summaries(&self, limit: usize) -> Vec<DbStatSummary> {
        let Ok(mut stmt) = self.conn.prepare(
            "\
            SELECT name, SUM(pgsize) AS bytes
            FROM dbstat
            GROUP BY name
            ORDER BY bytes DESC
            LIMIT ?1
            ",
        ) else {
            return Vec::new();
        };

        let Ok(rows) = stmt.query_map([limit as i64], |row| {
            let top_table: String = row.get(0)?;
            let bytes: i64 = row.get(1)?;
            Ok(DbStatSummary {
                top_table,
                mb: bytes as f64 / (1024.0 * 1024.0),
            })
        }) else {
            return Vec::new();
        };

        rows.filter_map(|row| row.ok()).collect()
    }
```

- [x] **Step 3: Emit dbstat top rows**

In `ProfileRunVisitor::emit_dbstat`, replace the body with:

```rust
        let summaries = self.writer.dbstat_summaries(8);
        if summaries.is_empty() {
            self.perf_log.event("phase=sqlite.dbstat unavailable=1");
            return;
        }

        for (index, summary) in summaries.iter().enumerate() {
            self.perf_log.event(&format!(
                "phase=sqlite.dbstat rank={} table={} mb={:.3}",
                index + 1,
                summary.top_table,
                summary.mb
            ));
        }
```

- [x] **Step 4: Run dbstat test**

Run:

```bash
cargo test perf_log_dbstat_is_opt_in -- --nocapture
```

Expected:

```text
PASS
```

- [x] **Step 5: Commit**

```bash
git add src/lib.rs src/sqlite/writer.rs tests/perf_log.rs
git commit -m "feat(perf): log dbstat table ranking"
```

---

### Task 8: Documentation Updates

**Files:**
- Modify: `docs/PERFORMANCE.md`
- Modify: `docs/CLI_CONTRACT.md`
- Modify: `docs/plans/PROFILE_JSON_REFS_V0_1_1_PERF_DIAGNOSTIC_DENSITY.md`

- [x] **Step 1: Update performance release status**

In `docs/PERFORMANCE.md`, change:

```text
Release status: `v0.1.0-rc.2`.
```

to:

```text
Release status: `v0.1.1` planned performance-diagnostic patch over the shipped `v0.1.0` baseline.
```

- [x] **Step 2: Add v0.1.1 note to Performance Log section**

Append this paragraph to section 9:

```markdown
`v0.1.1` does not add CLI options or stdout output. It increases diagnostic density inside the existing requested perf-log stream by adding scan accumulator sizes, SQLite per-table flush timings, prune-family timings, summary/index timings, and SQLite file-size facts.
```

- [x] **Step 3: Update CLI_CONTRACT example without changing syntax**

In `docs/CLI_CONTRACT.md`, keep the existing command example and add representative lines to the stderr example:

```text
[perf] t=12.345 phase=scan.accumulators pending_shapes=120 pending_shape_fields=940 pending_object_samples=320 pending_value_samples=10000 field_value_accumulators=940
[perf] t=12.456 phase=sqlite.flush.value_samples elapsed_ms=58 rows=10000
[perf] t=12.470 phase=sqlite.prune.value_priority elapsed_ms=14 fields=87
[perf] t=41.200 phase=sqlite.summary.counts elapsed_ms=3 canonical_paths=42 site_paths=42 shapes=120 field_profiles=940 stored_values=4000
[perf] t=41.220 phase=sqlite.size profile_sqlite_bytes=12345678 profile_sqlite_wal_bytes=0 profile_sqlite_shm_bytes=0
```

- [x] **Step 4: Verify release note draft**

Ensure this plan ends with exactly this release note and commit draft section:

```markdown
## Release Note Draft

`profile-json-refs v0.1.1` improves the diagnostic density of the existing `--perf-log` stream. The release adds scan accumulator sizes, per-table SQLite flush timings, prune-family timings, summary/index timings, SQLite file-size facts, and richer opt-in dbstat table ranking. It does not add CLI options, YAML fields, stdout output, or SQLite artifact tables.

## Commit Message Drafts

1. test(perf): require v0.1.1 diagnostic density
2. feat(perf): log scan accumulator density
3. feat(perf): log sqlite flush stages
4. feat(perf): log sqlite prune families
5. feat(perf): log sqlite finalization density
6. feat(perf): log dbstat table ranking
7. docs(perf): document v0.1.1 diagnostic density
```

- [x] **Step 5: Commit**

```bash
git add docs/PERFORMANCE.md docs/CLI_CONTRACT.md docs/plans/PROFILE_JSON_REFS_V0_1_1_PERF_DIAGNOSTIC_DENSITY.md
git commit -m "docs(perf): document v0.1.1 diagnostic density"
```

---

## Verification

Run focused tests:

```bash
cargo test perf_log -- --nocapture
```

Expected:

```text
all perf_log tests pass
```

Run output contract tests:

```bash
cargo test output_contract -- --nocapture
```

Expected:

```text
all output_contract tests pass
stdout still contains no [perf] lines
```

Run the non-heavy regression suite:

```bash
cargo test -- --skip rc2_diagnose_script_enforces_performance_safe_sample_contract
```

Expected:

```text
all selected tests pass
```

Run whitespace check:

```bash
git diff --check
```

Expected:

```text
no output
exit 0
```

---

## Implementation Notes

- Prefer additive `[perf] t=... phase=...` events over changing existing bucket names.
- Do not make tests assert exact elapsed values.
- Avoid collecting before/after table row counts in hot paths unless they are already available or cheap. Row counts for current chunks are enough for v0.1.1.
- Do not run `EXPLAIN QUERY PLAN` in v0.1.1. It needs a separate opt-in gate, and new options are out of scope.
- If a diagnostic would require a full table scan, leave it out unless it is behind existing `--perf-log-dbstat`.
- Keep normal stdout exactly as v0.1.0.

---

## Release Note Draft

`profile-json-refs v0.1.1` improves the diagnostic density of the existing `--perf-log` stream. The release adds scan accumulator sizes, per-table SQLite flush timings, prune-family timings, summary/index timings, SQLite file-size facts, and richer opt-in dbstat table ranking. It does not add CLI options, YAML fields, stdout output, or SQLite artifact tables.

## Commit Message Drafts

1. test(perf): require v0.1.1 diagnostic density
2. feat(perf): log scan accumulator density
3. feat(perf): log sqlite flush stages
4. feat(perf): log sqlite prune families
5. feat(perf): log sqlite finalization density
6. feat(perf): log dbstat table ranking
7. docs(perf): document v0.1.1 diagnostic density
