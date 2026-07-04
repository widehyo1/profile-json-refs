# profile-json-refs rc.2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the tracked Rust implementation in line with the `v0.1.0-rc.2` documentation contract and make the script-backed regression checks pass.

**Architecture:** Keep `profile-json-refs` as a downstream profile-fact layer over stable `dump-json-refs` artifacts. The fix is local to profile sampling, config, SQLite pruning, and perf logging; do not move refs traversal or raw extraction responsibility into this repository.

**Tech Stack:** Rust 2024, clap, serde_yaml, rusqlite bundled SQLite, shell regression scripts, cargo integration tests.

---

## Required Reading

- `/home/widehyo/gitclone/dump-json-refs/docs/references/REFS_REFERENCE_IMPLEMENTATION.md`
- `docs/PROFILE_JSON_REFS_PERF_DIAGNOSIS.md`
- `docs/PROFILE_REGRESSION_AND_PERFORMANCE_PLAN.md`
- `docs/PERFORMANCE.md`
- `docs/CLI_CONTRACT.md`
- `docs/SPEC.md`

Do not change the upstream refs contract. All changes belong to this repo's profile-fact layer.

---

## Current Red Tests

Run before implementation:

```bash
cargo test rc2_diagnose_script_enforces_performance_safe_sample_contract -- --nocapture
```

Expected current failure:

```text
heavy_hitter_context_rows > 0
field_profiles_exceeding_hh_context_bound > 0
value_priority_violations > 0
```

Run the rest of the suite without the red guard to detect unrelated breakage:

```bash
cargo test -- --skip rc2_diagnose_script_enforces_performance_safe_sample_contract
```

Expected current result:

```text
all non-rc2-guard tests pass
```

---

## Files To Modify

```text
src/cli.rs
  Add --perf-log-file and --perf-log-dbstat.

src/config.rs
  Add perf-log destination/dbstat config, rc.2 defaults, YAML fields, and CLI precedence.

src/perf/timer.rs
  Replace final-only bucket storage with an event writer that can flush during execution while preserving final buckets.

src/main.rs
  Route perf output to stderr or --perf-log-file and emit final stdout.summary timing.

src/lib.rs
  Emit scan/flush perf progress events from the long-running path.

src/field/accumulator.rs
  Stop generating heavy_hitter_context rows during scan.

src/value/sample.rs
  Remove scan-time heavy_hitter_context state and perform priority materialization only after top-K admission.

src/shape/sample.rs
  Perform object priority sample materialization only after top-K admission.

src/sketch/priority.rs
  Add reusable should_accept(priority) behavior.

src/sqlite/writer.rs
  Skip heavy_hitter_context prune when disabled and replace whole-table prune with touched-key scoped prune where practical.

tests/cli_contract.rs
tests/value_samples.rs
tests/perf_log.rs
tests/perf_smoke.rs
tests/profile_fixtures.rs
tests/sketches.rs
tests/sqlite_writer.rs
  Update or add behavior tests before implementation changes.

scripts/diagnose_profile_sqlite.sh
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
  Keep script assertions aligned with the final contract.
```

---

### Task 1: Lock Down rc.2 CLI and Config Defaults

**Files:**
- Modify: `tests/cli_contract.rs`
- Modify: `src/cli.rs`
- Modify: `src/config.rs`

- [x] **Step 1: Add failing config-default assertions**

In `tests/cli_contract.rs`, extend `defaults_use_contract_paths_and_auto_format`:

```rust
    assert_eq!(config.sampling.value_json_limit_bytes, 1024);
    assert_eq!(config.sampling.parent_object_json_limit_bytes, 1024);
    assert_eq!(config.sampling.value_priority_limit_per_field_profile, 4);
    assert_eq!(config.sampling.heavy_hitter_context_sample_limit, 0);
    assert_eq!(config.value_profile.value_text_limit_bytes, 512);
    assert!(config.perf_log_file.is_none());
    assert!(!config.perf_log_dbstat);
```

- [x] **Step 2: Add failing perf option parse test**

Add this test to `tests/cli_contract.rs`:

```rust
#[test]
fn perf_log_file_and_dbstat_parse_into_config() {
    let config = parse_config(&[
        "profile-json-refs",
        "data.jsonl",
        "--perf-log",
        "--perf-log-file",
        "target/tmp/perf.log",
        "--perf-log-dbstat",
    ]);

    assert!(config.perf_log);
    assert_eq!(config.perf_log_file, Some(PathBuf::from("target/tmp/perf.log")));
    assert!(config.perf_log_dbstat);
}
```

- [x] **Step 3: Add failing YAML parse test**

Add to `tests/cli_contract.rs`:

```rust
#[test]
fn perf_file_and_dbstat_yaml_fields_are_supported() {
    let dir = unique_temp_dir("perf-yaml");
    let config_path = dir.join("profile.yaml");
    let perf_path = dir.join("perf.log");
    fs::write(
        &config_path,
        format!(
            r#"
perf:
  log: true
  file: {}
  dbstat: true
"#,
            perf_path.display()
        ),
    )
    .expect("write config");

    let config = parse_config(&[
        "profile-json-refs",
        "data.json",
        "--config",
        config_path.to_str().expect("utf8 path"),
    ]);

    assert!(config.perf_log);
    assert_eq!(config.perf_log_file, Some(perf_path));
    assert!(config.perf_log_dbstat);
}
```

- [x] **Step 4: Run tests and verify red**

Run:

```bash
cargo test defaults_use_contract_paths_and_auto_format perf_log_file_and_dbstat_parse_into_config perf_file_and_dbstat_yaml_fields_are_supported --test cli_contract
```

Expected:

```text
FAIL until src/cli.rs and src/config.rs are updated
```

- [x] **Step 5: Implement CLI fields**

In `src/cli.rs`, add:

```rust
    /// Write perf-log events to a file instead of stderr.
    #[arg(long = "perf-log-file", value_name = "FILE")]
    pub perf_log_file: Option<PathBuf>,

    /// Include optional SQLite dbstat diagnostics in perf-log output.
    #[arg(long = "perf-log-dbstat")]
    pub perf_log_dbstat: bool,
```

- [x] **Step 6: Implement config fields**

In `src/config.rs`, add to `ProfileConfig`:

```rust
    pub perf_log_file: Option<PathBuf>,
    pub perf_log_dbstat: bool,
```

Initialize in `from_cli`:

```rust
            perf_log_file: None,
            perf_log_dbstat: false,
```

Extend `PerfConfig`:

```rust
struct PerfConfig {
    log: Option<bool>,
    file: Option<PathBuf>,
    dbstat: Option<bool>,
}
```

In `apply_file_config`, apply all fields:

```rust
        if let Some(perf) = file.perf {
            if let Some(log) = perf.log {
                self.perf_log = log;
            }
            if let Some(file) = perf.file {
                self.perf_log_file = Some(file);
                self.perf_log = true;
            }
            if let Some(dbstat) = perf.dbstat {
                self.perf_log_dbstat = dbstat;
            }
        }
```

In `apply_cli_overrides`:

```rust
        if let Some(file) = args.perf_log_file {
            self.perf_log_file = Some(file);
            self.perf_log = true;
        }
        if args.perf_log_dbstat {
            self.perf_log_dbstat = true;
        }
```

- [x] **Step 7: Implement rc.2 defaults**

In `SamplingConfig::default()`:

```rust
            value_json_limit_bytes: 1024,
            parent_object_json_limit_bytes: 1024,
            value_priority_limit_per_field_profile: 4,
            heavy_hitter_context_sample_limit: 0,
```

In `ValueProfileConfig::default()`:

```rust
            value_text_limit_bytes: 512,
```

- [x] **Step 8: Verify green**

Run:

```bash
cargo test --test cli_contract
```

Expected:

```text
all cli_contract tests pass
```

---

### Task 2: Stop Scan-Time heavy_hitter_context Rows

**Files:**
- Modify: `tests/value_samples.rs`
- Modify: `tests/perf_smoke.rs`
- Modify: `src/field/accumulator.rs`
- Modify: `src/value/sample.rs`
- Modify: `src/sqlite/writer.rs`

- [x] **Step 1: Replace the old heavy hitter context test**

In `tests/value_samples.rs`, replace `heavy_hitter_context_samples_are_bounded` with:

```rust
#[test]
fn heavy_hitter_context_samples_are_not_emitted_in_rc2() {
    let mut config = profile_config();
    config.sampling.heavy_hitter_context_sample_limit = 4;
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    for index in 0..10 {
        accumulator.observe(
            index,
            "$.field",
            &json!("hot"),
            &json!({"field": "hot"}),
            &config,
        );
    }

    let rows = accumulator.finish(&config).value_samples;
    let context_count = rows
        .iter()
        .filter(|row| row.sample_kind == ValueSampleKind::HeavyHitterContext)
        .count();
    assert_eq!(context_count, 0);
}
```

- [x] **Step 2: Update the profile_config helper**

In `tests/value_samples.rs`, change:

```rust
            heavy_hitter_context_sample_limit: 1,
```

to:

```rust
            heavy_hitter_context_sample_limit: 0,
```

- [x] **Step 3: Update perf smoke expectations**

In `tests/perf_smoke.rs`, set fixture config:

```yaml
    heavy_hitter_context_sample_limit: 0
```

Replace the `<= 1` assertion with:

```rust
    assert_eq!(
        max_group_count(
            &conn,
            "\
            SELECT MAX(cnt)
            FROM (
                SELECT COUNT(*) AS cnt
                FROM prof_field_value_sample
                WHERE sample_kind = 'heavy_hitter_context'
                GROUP BY field_profile_id, value_hash
            )
            "
        ),
        0
    );
```

- [x] **Step 4: Run tests and verify red**

Run:

```bash
cargo test heavy_hitter_context_samples_are_not_emitted_in_rc2 large_finite_jsonl_run_keeps_persisted_samples_bounded -- --nocapture
```

Expected:

```text
FAIL until scan-time heavy_hitter_context generation is removed
```

- [x] **Step 5: Remove scan-time generation**

In `src/field/accumulator.rs`, delete this block from `FieldValueAccumulator::observe`:

```rust
        if self.heavy_hitters.contains_key(&key) {
            self.heavy_hitter_values.insert(key.clone(), value.clone());
            self.value_samples.observe_heavy_hitter_context(
                document_index,
                source_path,
                &self.field_profile_id,
                value,
                parent_object,
                config,
            );
        }
```

Replace it with:

```rust
        if self.heavy_hitters.contains_key(&key) {
            self.heavy_hitter_values.insert(key.clone(), value.clone());
        }
```

Delete the `self.value_samples.retain_heavy_hitter_keys(&active_keys);` call.

- [x] **Step 6: Remove unused context state**

In `src/value/sample.rs`, remove:

```rust
    heavy_hitter_context_limit: usize,
    heavy_hitter_context: HashMap<ValueKey, Vec<ValueSampleRow>>,
```

Change the constructor to accept the second argument for API stability but not store it:

```rust
    pub fn new(priority_limit: usize, _heavy_hitter_context_limit: usize) -> Self {
        Self {
            priority_limit,
            seen_once: HashSet::new(),
            priority: PrioritySampler::new(priority_limit),
            rows: Vec::new(),
        }
    }
```

Remove these methods:

```rust
observe_heavy_hitter_context
retain_heavy_hitter_keys
```

Remove context row extension from `rows()` and context counting from `pending_row_count()`.

- [x] **Step 7: Skip disabled prune**

In `src/sqlite/writer.rs`, update `flush_chunk`:

```rust
        if self.heavy_hitter_context_sample_limit > 0 {
            self.prune_heavy_hitter_context_samples()?;
        }
```

- [x] **Step 8: Verify green**

Run:

```bash
cargo test --test value_samples
cargo test --test perf_smoke
cargo test rc2_diagnose_script_enforces_performance_safe_sample_contract -- --nocapture
```

Expected:

```text
value_samples and perf_smoke pass
rc2_diagnose_script_enforces_performance_safe_sample_contract no longer reports heavy_hitter_context risk
```

If the rc2 diagnostic test still reports `value_priority_violations`, continue to Task 3.

---

### Task 3: Defer Priority Sample Materialization Until Admission

**Files:**
- Modify: `src/sketch/priority.rs`
- Modify: `src/value/sample.rs`
- Modify: `src/shape/sample.rs`
- Test: `tests/value_samples.rs`
- Test: `tests/object_samples.rs`

- [x] **Step 1: Add PrioritySampler admission tests**

Add to `tests/sketches.rs`:

```rust
#[test]
fn priority_sampler_reports_admission_before_materialization() {
    let mut sampler = profile_json_refs::sketch::priority::PrioritySampler::new(2);

    assert!(sampler.should_accept(20));
    sampler.push(20, "twenty");
    assert!(sampler.should_accept(10));
    sampler.push(10, "ten");
    assert!(sampler.should_accept(5));
    assert!(!sampler.should_accept(30));
}
```

- [x] **Step 2: Run test and verify red**

Run:

```bash
cargo test priority_sampler_reports_admission_before_materialization --test sketches
```

Expected:

```text
FAIL because should_accept is missing
```

- [x] **Step 3: Implement should_accept**

In `src/sketch/priority.rs`, add:

```rust
    pub fn should_accept(&self, priority: u64) -> bool {
        if self.limit == 0 {
            return false;
        }
        if self.items.len() < self.limit {
            return true;
        }
        self.items
            .iter()
            .map(|item| item.priority)
            .max()
            .is_some_and(|worst| priority < worst)
    }
```

- [x] **Step 4: Use should_accept for value samples**

In `src/value/sample.rs`, change priority enqueue logic to:

```rust
        if self.priority_limit > 0 {
            let priority = sample_priority(field_profile_id, document_index, source_path);
            if self.priority.should_accept(priority) {
                self.priority.push(
                    priority,
                    make_value_sample_row(
                        ValueSampleKind::PrioritySample,
                        &observation,
                        Some(priority),
                    ),
                );
            }
        }
```

- [x] **Step 5: Use admission check for object samples**

In `src/shape/sample.rs`, add to `TopK`:

```rust
    fn should_accept(&self, priority: u64) -> bool {
        if self.candidates.len() < self.limit {
            return true;
        }
        self.candidates
            .iter()
            .map(|candidate| candidate.priority)
            .max()
            .is_some_and(|worst| priority < worst)
    }
```

Then in `enqueue_priority`, get or create `top_k` before row materialization:

```rust
        let top_k = self
            .priority
            .entry((scope, key.to_string()))
            .or_insert_with(|| TopK::new(limit));
        if !top_k.should_accept(priority) {
            return Ok(());
        }
        let row = make_object_sample_row(
            scope,
            key,
            ObjectSampleKind::PrioritySample,
            observation,
            Some(priority),
        )?;
        top_k.push(ObjectSampleCandidate { priority, row });
```

- [x] **Step 6: Verify rc.2 diagnostic bound**

Run:

```bash
cargo test rc2_diagnose_script_enforces_performance_safe_sample_contract -- --nocapture
```

Expected:

```text
PASS once value priority rows are bounded to rc.2 defaults and heavy_hitter_context rows are zero
```

---

### Task 4: Implement Perf Log File, Progress Events, and dbstat Flag

**Files:**
- Modify: `tests/perf_log.rs`
- Modify: `src/perf/timer.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Modify: `src/sqlite/writer.rs`

- [x] **Step 1: Add perf-log-file test**

Add to `tests/perf_log.rs`:

```rust
#[test]
fn perf_log_file_writes_events_outside_stderr() {
    let fixture = basic_fixture("perf-log-file", r#"{"id":1}"#, false);
    let perf_log = fixture.dir.join("perf.log");

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
        "--perf-log-file".to_string(),
        perf_log.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!stderr(&output).contains("[perf]"));
    let perf = std::fs::read_to_string(&perf_log).expect("read perf log");
    assert!(perf.contains("[perf]"));
    assert!(perf.contains("phase=scan.progress") || perf.contains("scan.read_parse_walk"));
}
```

- [x] **Step 2: Add dbstat opt-in test**

Add to `tests/perf_log.rs`:

```rust
#[test]
fn perf_log_dbstat_is_opt_in() {
    let fixture = basic_fixture("perf-log-dbstat", r#"{"id":1}"#, false);

    let without = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);
    assert!(without.status.success(), "stderr: {}", stderr(&without));
    assert!(!stderr(&without).contains("phase=sqlite.dbstat"));

    let with = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
        "--perf-log-dbstat".to_string(),
    ]);
    assert!(with.status.success(), "stderr: {}", stderr(&with));
    assert!(stderr(&with).contains("phase=sqlite.dbstat"));
}
```

- [x] **Step 3: Run tests and verify red**

Run:

```bash
cargo test --test perf_log
```

Expected:

```text
FAIL until perf destination and progress/dbstat events are implemented
```

- [x] **Step 4: Implement a flushed perf writer**

In `src/perf/timer.rs`, keep `PerfBucket` for final buckets and add a writer that owns either stderr or a file. Use `Box<dyn std::io::Write>`:

```rust
pub enum PerfDestination {
    Stderr,
    File(std::path::PathBuf),
}
```

Expose a constructor returning `Result<PerfLog>` so file creation errors can fail clearly before scanning.

Every event write must call `flush()`.

- [x] **Step 5: Emit progress events**

Emit at least these lines when `--perf-log` is active:

```text
[perf] t=<seconds> phase=scan.progress documents=<n> objects=<n> arrays=<n> scalars=<n>
[perf] t=<seconds> phase=flush.chunk shapes=<n> fields=<n> object_samples=<n> value_samples=<n>
```

It is acceptable for small tests to emit one progress event at final scan completion and one flush event per chunk.

- [x] **Step 6: Emit dbstat only when requested**

When `perf_log_dbstat` is true, query SQLite `dbstat` after finalization and emit:

```text
[perf] t=<seconds> phase=sqlite.dbstat top_table=<name> mb=<value>
```

If `dbstat` is unavailable, emit:

```text
[perf] t=<seconds> phase=sqlite.dbstat unavailable=1
```

- [x] **Step 7: Verify green**

Run:

```bash
cargo test --test perf_log
```

Expected:

```text
all perf_log tests pass
```

---

### Task 5: Scope SQLite Prune Work

**Files:**
- Modify: `src/sqlite/writer.rs`
- Test: `tests/sqlite_writer.rs`

- [x] **Step 1: Add a writer regression test**

Add a test in `tests/sqlite_writer.rs` that flushes multiple chunks with repeated priority samples for a small set of field profiles and verifies no priority rows exceed the configured limit after each flush:

```rust
fn priority_value_sample_row(
    id: &str,
    field_profile_id: &str,
    priority: u64,
    document_index: u64,
) -> ValueSampleRow {
    ValueSampleRow {
        value_sample_id: id.to_string(),
        field_profile_id: field_profile_id.to_string(),
        value_hash: Some(format!("value-{priority}")),
        sample_kind: ValueSampleKind::PrioritySample,
        document_index,
        source_path: "$.id".to_string(),
        value_json: Some(priority.to_string()),
        value_json_truncated: false,
        parent_object_json: Some(format!(r#"{{"id":{priority}}}"#)),
        parent_object_json_truncated: false,
        sample_priority: Some(priority),
        sample_rank: None,
    }
}

#[test]
fn value_priority_prune_keeps_rows_bounded_across_chunk_flushes() {
    let dir = unique_temp_dir("value-priority-prune");
    let out = dir.join("profile.sqlite");
    let mut config = test_config(&out);
    config.sampling.value_priority_limit_per_field_profile = 2;
    let mut writer = ProfileWriter::open(&out, &config).expect("open writer");

    writer
        .flush_chunk(ProfileChunk {
            shapes: vec![shape_row()],
            value_samples: vec![
                priority_value_sample_row("priority-30", "field-1", 30, 0),
                priority_value_sample_row("priority-20", "field-1", 20, 1),
            ],
            ..ProfileChunk::default()
        })
        .expect("first flush");

    writer
        .flush_chunk(ProfileChunk {
            value_samples: vec![
                priority_value_sample_row("priority-10", "field-1", 10, 2),
                priority_value_sample_row("priority-40", "field-1", 40, 3),
            ],
            ..ProfileChunk::default()
        })
        .expect("second flush");

    let count: u64 = writer
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM prof_field_value_sample WHERE sample_kind = 'priority_sample'",
            [],
            |row| row.get(0),
        )
        .expect("query priority sample count");
    assert_eq!(count, 2);

    let kept: Vec<String> = writer
        .connection()
        .prepare(
            "SELECT value_sample_id FROM prof_field_value_sample WHERE sample_kind = 'priority_sample' ORDER BY sample_rank",
        )
        .expect("prepare kept query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query kept rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect kept rows");

    assert_eq!(kept, vec!["priority-10".to_string(), "priority-20".to_string()]);
}
```

- [x] **Step 2: Run test and verify current behavior**

Run:

```bash
cargo test value_priority_prune_keeps_rows_bounded_across_chunk_flushes --test sqlite_writer
```

Expected:

```text
PASS under current behavior, then keep passing after prune refactor
```

- [x] **Step 3: Implement scoped prune without changing results**

Keep correctness identical. Refactor `ProfileWriter::flush_chunk` to collect touched keys from `chunk.object_samples` and `chunk.value_samples`:

```rust
let touched_value_fields: BTreeSet<String> = chunk
    .value_samples
    .iter()
    .filter(|row| row.sample_kind == ValueSampleKind::PrioritySample)
    .map(|row| row.field_profile_id.clone())
    .collect();
```

Use touched keys in prune SQL. If the touched set is empty, skip that prune.

- [x] **Step 4: Verify no behavior drift**

Run:

```bash
cargo test --test sqlite_writer
cargo test rc2_diagnose_script_enforces_performance_safe_sample_contract -- --nocapture
```

Expected:

```text
sqlite_writer passes
rc2 diagnostic remains green
```

---

### Task 6: Final Verification

**Files:**
- No new files expected unless a test fixture is needed.

- [x] **Step 1: Format**

Run:

```bash
cargo fmt --check
```

Expected:

```text
exit 0
```

- [x] **Step 2: Full cargo test**

Run:

```bash
cargo test
```

Expected:

```text
all tests pass, including rc2_diagnose_script_enforces_performance_safe_sample_contract
```

- [x] **Step 3: Script syntax**

Run:

```bash
bash -n scripts/diagnose_profile_sqlite.sh
bash -n scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

Expected:

```text
exit 0 for both commands
```

- [x] **Step 4: External regression**

Build release binary:

```bash
cargo build --release
```

Run the manual regression if `dump-json-refs` is available:

```bash
PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
DUMP_JSON_REFS_BIN=dump-json-refs \
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

Expected:

```text
PASS v0.1.0-rc.2 performance regression
```

- [x] **Step 5: Documentation consistency check**

Run:

```bash
rg -n "regression_profile_json_refs_v0_1_1_patch|v0\\.1\\.1" docs scripts tests README.md -g '!docs/plans/PROFILE_JSON_REFS_RC2_IMPLEMENTATION_WORK.md'
```

Expected:

```text
no matches
```

Run:

```bash
rg -n "v0\\.1\\.0-rc\\.2|heavy_hitter_context_sample_limit|--perf-log-file|--perf-log-dbstat" docs README.md src tests scripts
```

Expected:

```text
matches show rc.2 docs, code, tests, and scripts agree on the new contract
```

---

## Commit Guidance

Use small commits:

```bash
git add src/cli.rs src/config.rs tests/cli_contract.rs
git commit -m "feat(cli): add rc2 perf config and defaults"

git add src/field/accumulator.rs src/value/sample.rs src/sqlite/writer.rs tests/value_samples.rs tests/perf_smoke.rs
git commit -m "fix(samples): disable scan-time heavy hitter context"

git add src/sketch/priority.rs src/value/sample.rs src/shape/sample.rs tests/sketches.rs tests/value_samples.rs tests/object_samples.rs
git commit -m "perf(samples): defer priority sample materialization"

git add src/perf src/main.rs src/lib.rs tests/perf_log.rs
git commit -m "feat(perf): emit flushed rc2 progress logs"

git add src/sqlite/writer.rs tests/sqlite_writer.rs
git commit -m "perf(sqlite): scope sample prune work"

git add docs scripts tests/profile_fixtures.rs README.md
git commit -m "docs(rc2): document performance regression workflow"
```

Before committing, ensure release/commit draft sections in docs match the actual commit scope.
