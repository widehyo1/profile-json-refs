# Sourcemap

This document maps repository responsibilities to files.

Update this file whenever documented files or major source modules are added, removed, or renamed.

---

## Root Files

```text
README.md
  Repository overview, purpose, basic usage, and non-goals.

AGENTS.md
  Instructions for implementation agents and documentation discipline.

Cargo.toml
  Rust package metadata and dependencies.

Cargo.lock
  Locked dependency versions for reproducible builds.
```

---

## Documentation

```text
docs/SPEC.md
  Main specification and `v0.1.0-rc.2` contract entrypoint.

docs/CLI_CONTRACT.md
  CLI arguments, rc.2 defaults, config YAML, stdout/stderr/perf-log behavior, stdin policy, and exit behavior.

docs/DATA_MODEL.md
  Physical prof_* SQLite tables, identifiers, keys, indexes, and column semantics.

docs/POPULATION_RULES.md
  Source scan events, refs resolution, heterogeneous object array handling, accumulator behavior, chunk flush, and table population.

docs/PROBABILISTIC_DS.md
  Exact fallback, HyperLogLog, Space-Saving, deterministic priority sampling, heavy_hitter_context default-off policy, and count semantics.

docs/PERFORMANCE.md
  Streaming, bounded memory, deferred materialization, chunk flush, heterogeneous-array cardinality, rc.2 value-sampling safety, perf-log progress, and SQLite write policy.

docs/REFERENCES.md
  Upstream dump-json-refs artifacts and refs tables consumed by this project.

docs/PROFILE_IMPLEMENTATION_MASTER_PLAN.md
  Phase-level implementation plan and commit boundaries.

docs/plans/PROFILE_JSON_REFS_RC2_IMPLEMENTATION_WORK.md
  Working implementation checklist for the rc.2 performance-safe code changes and regression gates.

docs/SOURCEMAP.md
  This file.
```

---

## Suggested Source Layout

```text
src/main.rs
  Thin binary entrypoint.

src/lib.rs
  Public library entrypoint for integration into a later combined tool.

src/cli.rs
  CLI parser and CLI-to-config conversion.

src/config.rs
  Built-in defaults, YAML config loading, and precedence handling.

src/error.rs
  Error and warning types.
```

---

## Refs Adapter

```text
src/refs/mod.rs
  Refs module entrypoint.

src/refs/sqlite.rs
  SQLite connection and query helpers for refs/schemas.sqlite.

src/refs/contract.rs
  Required upstream table/column checks.

src/refs/site.rs
  schema_site_* loading and site-level structural seed handling.

src/refs/resolver.rs
  Mapping source scan events to canonical_path, site_path, and schema_path.
```

---

## v0.1.0-rc.2 Performance-Safe Candidate

```text
docs/SPEC.md
  rc.2 contract: heavy_hitter_context disabled by default, safer value context defaults, perf-log options.

docs/CLI_CONTRACT.md
  --perf-log-file, --perf-log-dbstat, and rc.2 YAML defaults.

docs/PERFORMANCE.md
  Large-input value-sampling fix and incremental perf-log requirements.

docs/PROFILE_JSON_REFS_PERF_DIAGNOSIS.md
  Diagnosis that motivated rc.2: prof_field_value_sample / heavy_hitter_context growth.

docs/PROFILE_REGRESSION_AND_PERFORMANCE_PLAN.md
  rc.2 regression expectations and large-input smoke checks.
```

---

## Source Scanner

```text
src/scan/mod.rs
  Scanner module entrypoint.

src/scan/json.rs
  JSON file scanner.

src/scan/jsonl.rs
  JSONL file scanner.

src/scan/path.rs
  Source path representation.

src/scan/visitor.rs
  Traversal event visitor interface.
```

---

## Shape and Object Sampling

```text
src/shape/mod.rs
  Shape module entrypoint.

src/shape/id.rs
  shape_id generation.

src/shape/token.rs
  Field/type token representation for efficient identity.

src/shape/field_set.rs
  Field set construction and stable ordering.

src/shape/type_set.rs
  Type set construction and stable ordering.

src/shape/accumulator.rs
  Shape counters and shape-level aggregation.

src/shape/sample.rs
  prof_object_sample row construction, first_seen / first_non_empty handling, and priority sample keys.
```

---

## Heterogeneous Object Arrays

```text
docs/SPEC.md

0.1.0 policy: object elements in heterogeneous arrays are handled through existing prof_shape rows; dedicated array profiling is deferred.

docs/POPULATION_RULES.md

canner behavior for object elements inside arrays and deferred array-specific statistics.

docs/DATA_MODEL.md

rof_shape semantics for multiple shapes under one array site.

docs/PERFORMANCE.md

hape/sample cardinality risk from heterogeneous object arrays.

docs/PROFILE_IMPLEMENTATION_MASTER_PLAN.md

hase acceptance criteria and fixture requirements for heterogeneous object arrays.
```

---

## Field and Value Profiling

```text
src/field/mod.rs
  Field module entrypoint.

src/field/id.rs
  field_profile_id generation.

src/field/accumulator.rs
  Field-level counters and sketch updates.

src/field/summary.rs
  prof_field_summary row construction.

src/field/value.rs
  prof_field_value and prof_field_value_sample row construction.

src/value/mod.rs
  Value module entrypoint.

src/value/identity.rs
  Stable value identity semantics.

src/value/interner.rs
  Optional value interning and hot-path identity optimization.

src/value/exact_counter.rs
  Bounded exact value distribution tracking.

src/value/display.rs
  Bounded value_text and JSON display materialization.

src/value/special.rs
  null-only, empty-object-only, empty-array-only, and non-empty helpers.
```

---

## Sketches and Bounded Algorithms

```text
src/sketch/mod.rs
  Sketch module entrypoint.

src/sketch/hll.rs
  HyperLogLog distinct count.

src/sketch/space_saving.rs
  Heavy hitter candidate detection.

src/sketch/priority_sample.rs
  Deterministic priority sampling and merge/prune helpers.

src/sketch/hash.rs
  Shared hash helpers for sketches.
```

`src/sketch/reservoir.rs` is intentionally absent in v0.1.0.

---

## SQLite Output

```text
src/sqlite/mod.rs
  SQLite module entrypoint.

src/sqlite/schema.rs
  prof_* DDL creation.

src/sqlite/writer.rs
  Batched inserts, chunk flush, priority sample merge/prune, transactions, and index creation.
```

`src/sqlite/views.rs` is intentionally absent in v0.1.0 because SQLite views are not part of the output contract.

---

## Performance

```text
src/perf/mod.rs
  Performance module entrypoint.

src/perf/timer.rs
  Timing buckets for --perf-log.
```

---

## Tests

```text
tests/cli_config.rs
  CLI/config precedence and invalid config tests.

tests/sqlite_schema.rs
  prof_* DDL and no-extra-table checks.

tests/refs_contract.rs
  Required refs table checks.

tests/jsonl_scan.rs
  JSONL scanner fixtures.

tests/object_samples.rs
  canonical/site/field_set/type_set sample coverage and chunk flush behavior.

tests/heterogeneous_array.rs
  Heterogeneous object array fixture; verifies multiple prof_shape rows under one array site and no prof_array_* tables.

tests/value_profile.rs
  exact fallback, HLL, heavy hitter, and value sample behavior.

tests/perf_log.rs
  --perf-log stderr bucket contract.

tests/profile_fixtures.rs
  Fixture-driven profile assertions, checked-in script syntax checks, and the rc.2 diagnostic regression that invokes scripts/diagnose_profile_sqlite.sh.
```

---

## Scripts

```text
scripts/assert_profile_sqlite.sh
  Lightweight profile.sqlite SQL assertion helper.

scripts/diagnose_profile_sqlite.sh
  Read-only diagnostic and CI-style risk checker for profile.sqlite artifacts; used by the rc.2 cargo-level diagnostic regression.

scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
  Full external rc.2 performance-patch regression harness using a candidate profile-json-refs binary and dump-json-refs to generate refs.

scripts/make_fixture_refs.sh
  Minimal refs fixture helper for local/integration tests.

scripts/make_large_jsonl_fixture.py
  Large finite JSONL fixture generator for smoke/performance validation.
```
