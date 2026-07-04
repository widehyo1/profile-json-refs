# Profile Implementation Detail Plan

This document is the implementation index for `profile-json-refs` v0.1.0.

The master plan owns phase order and commit boundaries. This detail index maps each phase to focused code-level detail documents so the implementation instructions stay maintainable.

`profile-json-refs` consumes:

```text
- JSON or JSONL source file
- refs/schemas.sqlite produced by dump-json-refs
```

and writes:

```text
- profile.sqlite
```

v0.1.0 is a one-shot fact artifact. It has no run history, no manifest table, no warning table, no algorithm table, no SQLite views, no stdin support, and no `prof_array_*` tables.

---

## 1. Detail Document Map

```text
docs/plans/detail/PROFILE_DETAIL_01_CLI_CONFIG.md
  Phase 1 implementation details:
  CLI parser, config YAML, validation, precedence, input policy.

docs/plans/detail/PROFILE_DETAIL_02_SQLITE_REFS.md
  Phase 2 and Phase 3 implementation details:
  prof_* DDL, schema creation, refs DB contract validation, refs adapter.

docs/plans/detail/PROFILE_DETAIL_03_SCAN_SHAPE_SAMPLE.md
  Phase 4 and Phase 5 implementation details:
  streaming JSON/JSONL scanner, path events, shape identity, object samples,
  heterogeneous object array handling, first_seen/first_non_empty, priority sampling.

docs/plans/detail/PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md
  Phase 6, Phase 7, and Phase 8 implementation details:
  field_profile_id, field summaries, value identity, empty string handling,
  exact counter, HyperLogLog, Space-Saving, value samples.

docs/plans/detail/PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md
  Phase 9, Phase 10, and Phase 12 implementation details:
  SQLite writer batches, chunk flush, sample merge/prune SQL, stdout/stderr,
  perf-log buckets, memory/performance invariants.

docs/plans/detail/PROFILE_DETAIL_06_FIXTURES_TESTS.md
  Phase 11 implementation details:
  fixture layout, helper commands, SQL assertions, regression matrix,
  forbidden table/view checks.
```

---

## 2. Phase-to-Document Mapping

```text
Phase 0: repository skeleton and docs alignment
  - this index
  - docs/SOURCEMAP.md

Phase 1: CLI and config contract
  - PROFILE_DETAIL_01_CLI_CONFIG.md

Phase 2: SQLite schema writer
  - PROFILE_DETAIL_02_SQLITE_REFS.md

Phase 3: refs adapter
  - PROFILE_DETAIL_02_SQLITE_REFS.md

Phase 4: source scanner
  - PROFILE_DETAIL_03_SCAN_SHAPE_SAMPLE.md

Phase 5: shape identity and object samples
  - PROFILE_DETAIL_03_SCAN_SHAPE_SAMPLE.md

Phase 6: field/value accumulators
  - PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md

Phase 7: bounded exact distribution
  - PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md

Phase 8: sketches and priority sampling
  - PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md

Phase 9: SQLite writer integration
  - PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md

Phase 10: stdout, stderr warnings, and perf-log
  - PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md

Phase 11: fixtures and regression tests
  - PROFILE_DETAIL_06_FIXTURES_TESTS.md

Phase 12: performance validation
  - PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md
```

---

## 3. Global Code Invariants

The following invariants apply to all implementation phases.

```text
- Source input is file-only.
- `-` and stdin are rejected.
- The scanner must stream the source.
- The full source must not be materialized in memory.
- profile.sqlite contains only approved physical tables.
- No SQLite views are created in v0.1.0.
- No run/source/manifest/algorithm/warning tables are created.
- No `prof_array_*` tables are created in v0.1.0.
- Warnings go to stderr.
- Normal stdout is summary-only.
- `--perf-log` goes to stderr.
- Sample state must be chunk-flushed.
- The implementation must not keep unbounded per-key sample state in memory.
```

---

## 4. Commit Discipline

Each master-plan phase is the default source-control boundary.

A phase commit should include:

```text
- source files for that phase
- tests or fixtures introduced by that phase
- documentation updates required by that phase
```

Use conventional commit messages from the master plan unless a detail document explicitly splits a phase.

Do not batch unrelated phases into one commit.

---

## 5. Recommended Rust Module Layout

```text
src/
├── main.rs
├── lib.rs
├── cli.rs
├── config.rs
├── error.rs
├── refs/
│   ├── mod.rs
│   ├── sqlite.rs
│   ├── contract.rs
│   ├── site.rs
│   └── resolver.rs
├── scan/
│   ├── mod.rs
│   ├── json.rs
│   ├── jsonl.rs
│   ├── path.rs
│   └── visitor.rs
├── shape/
│   ├── mod.rs
│   ├── id.rs
│   ├── token.rs
│   ├── accumulator.rs
│   └── sample.rs
├── field/
│   ├── mod.rs
│   ├── id.rs
│   ├── accumulator.rs
│   └── summary.rs
├── value/
│   ├── mod.rs
│   ├── identity.rs
│   ├── interner.rs
│   ├── exact_counter.rs
│   └── display.rs
├── sketch/
│   ├── mod.rs
│   ├── hll.rs
│   ├── space_saving.rs
│   └── priority.rs
├── sqlite/
│   ├── mod.rs
│   ├── schema.rs
│   └── writer.rs
├── perf/
│   ├── mod.rs
│   └── timer.rs
└── util/
    ├── mod.rs
    ├── json_type.rs
    ├── hash.rs
    └── truncate.rs
```

---

## 6. Public Library Entry Point

`src/lib.rs` should expose one stable internal entry point used by `main.rs` and integration tests.

```rust
pub mod cli;
pub mod config;
pub mod error;
pub mod refs;
pub mod scan;
pub mod shape;
pub mod field;
pub mod value;
pub mod sketch;
pub mod sqlite;
pub mod perf;
pub mod util;

use crate::config::ProfileConfig;
use crate::error::Result;

pub struct ProfileReport {
    pub out_path: std::path::PathBuf,
    pub summary: SourceSummary,
    pub elapsed: std::time::Duration,
    pub warnings: Vec<ProfileWarning>,
}

pub struct SourceSummary {
    pub total_document_count: u64,
    pub total_object_count: u64,
    pub total_array_count: u64,
    pub total_scalar_count: u64,
    pub total_canonical_path_count: u64,
    pub total_site_path_count: u64,
    pub total_shape_count: u64,
    pub total_field_profile_count: u64,
    pub total_stored_value_count: u64,
}

pub fn run(config: ProfileConfig) -> Result<ProfileReport> {
    todo!("wired in Phase 9")
}
```

---

## 7. Done Criteria for the Detail Plan Set

The split detail docs are complete when they specify:

```text
- CLI structs and config structs
- config precedence and validation
- full v0.1.0 DDL
- required refs table validation
- source scanner events
- shape_id and field_profile_id construction
- prof_object_sample semantics
- first_seen and first_non_empty semantics
- heterogeneous object array behavior
- field summary counters including empty_string_count
- exact/approx value distribution transition
- HLL / Space-Saving / priority sampling responsibilities
- chunk flush and merge/prune SQL
- stdout/stderr/perf-log behavior
- regression fixture matrix
- SQL assertion examples
```
