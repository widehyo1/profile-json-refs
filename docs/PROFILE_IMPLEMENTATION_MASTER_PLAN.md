# Profile Implementation Master Plan

This plan defines the `v0.1.0-rc.2` implementation phases for `profile-json-refs`. rc.2 is the performance-safe release candidate intended to become final `v0.1.0` after regression and large-input smoke validation.

`profile-json-refs` consumes a JSON/JSONL source file and `refs/schemas.sqlite`, then writes `profile.sqlite`.

The implementation target is a one-shot fact artifact. There is no run history table, no manifest table, no warning table, no algorithm table, no SQLite views, and no stdin support in v0.1.0.

---

## 1. Scope

v0.1.0 includes:

```text
- file input for JSON and JSONL
- refs/schemas.sqlite consumption
- profile.sqlite output
- CLI defaults: --refs refs/schemas.sqlite, --out profile.sqlite
- config YAML support
- stdout summary from prof_source_summary + output path + elapsed time
- --quiet
- --perf-log to stderr
- --perf-log-file for flushed perf-log file output
- --perf-log-dbstat for opt-in SQLite size diagnostics
- refs contract validation
- source scanner
- shape-aware value profiling
  - heterogeneous object arrays through existing prof_shape rows
- prof_object_sample for canonical/site/field_set/type_set samples
- first_seen and first_non_empty samples
- deterministic priority sampling with chunk flush
- bounded exact value distribution for small field profiles
- HyperLogLog distinct count
- Space-Saving heavy hitters
- heavy_hitter_context disabled by default in rc.2
- safer value context defaults for large JSONL
- batched SQLite writer
- regression fixtures and performance checks
```

Out of scope:

```text
- stdin
- --strict
- report files
- SQLite views
- run/source/manifest/algorithm/warning tables
- final materialization decisions
- DBML / SQL DDL / parquet emission
- cross-snapshot lineage
  - dedicated prof_array_* tables and array-specific profiling
```

---

## 2. Commit Boundary

Each phase is the default source-control boundary.

After completing a phase, create one conventional commit for that phase unless the detail plan explicitly splits it.

Each phase commit should include:

```text
- implementation files for that phase
- tests or fixtures introduced by that phase
- documentation updates required by the implemented behavior
```

Do not batch unrelated phases into one commit.

---

## 3. Phase Overview

```text
Phase 0: repository skeleton and docs alignment
Phase 1: CLI and config contract
Phase 2: SQLite schema writer
Phase 3: refs adapter
Phase 4: source scanner
Phase 5: shape identity and object samples
Phase 6: field/value accumulators
Phase 7: bounded exact distribution
Phase 8: sketches and priority sampling
Phase 9: SQLite writer integration
Phase 10: stdout, stderr warnings, and perf-log
Phase 11: fixtures and regression tests
Phase 12: performance validation
```

---

## Phase 0: Repository Skeleton and Docs Alignment

Goal:

```text
Create the repository skeleton and align documentation files.
```

Work:

```text
- create Cargo project files
- add README.md and AGENTS.md
- add docs directory
- add SPEC, CLI_CONTRACT, DATA_MODEL, POPULATION_RULES, PROBABILISTIC_DS, PERFORMANCE, REFERENCES, SOURCEMAP
```

Done criteria:

```text
- cargo project builds with placeholder binary
- documentation structure matches SOURCEMAP
```

Default commit:

```text
chore(repo): initialize profile-json-refs skeleton
```

---

## Phase 1: CLI and Config Contract

Goal:

```text
Implement command-line parsing and config precedence.
```

Work:

```text
- implement file input only
- reject stdin and '-'
- implement --refs default refs/schemas.sqlite
- implement --out default profile.sqlite
- implement --jsonl
- implement --config YAML loading
- implement --quiet
- implement --perf-log flag
- implement --perf-log-file
- implement --perf-log-dbstat
- implement config validation
```

Done criteria:

```text
- CLI/config precedence follows CLI_CONTRACT.md
- unknown YAML keys fail early
- --strict is absent
- stdin is rejected
```

Default commit:

```text
feat(cli): implement config and input contract
```

---

## Phase 2: SQLite Schema Writer

Goal:

```text
Create the v0.1.0 prof_* schema.
```

Work:

```text
- create prof_source_summary
- create prof_object_sample
- create prof_shape
- create prof_shape_field
- create prof_field_summary
- create prof_field_value
- create prof_field_value_sample
- create indexes
- ensure no views are created
```

Done criteria:

```text
- schema test passes
- no prof_run/prof_manifest/prof_algorithm/prof_warning table exists
- no SQLite view exists
```

Default commit:

```text
feat(sqlite): create profile fact schema
```

---

## Phase 3: Refs Adapter

Goal:

```text
Load the required upstream refs contract.
```

Work:

```text
- open refs/schemas.sqlite
- validate required refs tables
- load schema_site_* structural seeds
- expose resolver inputs for scanner
- handle truncated presence-shape refs with stderr warnings
```

Done criteria:

```text
- missing required refs table fails clearly
- truncated presence shape does not stop execution
- adapter does not depend on undocumented internals
```

Default commit:

```text
feat(refs): load upstream refs contract
```

---

## Phase 4: Source Scanner

Goal:

```text
Stream JSON/JSONL source and emit traversal events.
```

Work:

```text
- implement JSON scanner
- implement JSONL scanner
- track document_index and source_path
- count objects, arrays, scalars, documents
- avoid full source materialization
```

Done criteria:

```text
- JSON fixture scans
- JSONL fixture scans
- stdin remains unsupported
- source summary counters are correct for basic fixtures
- object elements inside arrays are emitted as normal object scan events when refs context is available
```

Default commit:

```text
feat(scan): stream JSON and JSONL source files
```

---

## Phase 5: Shape Identity and Object Samples

Goal:

```text
Build detailed shape facts and source-backed object samples.
```

Work:

```text
- compute field_set_json and type_set_json
- compute field_set_hash and type_set_hash
- compute shape_id
- populate prof_shape
- populate prof_object_sample for canonical_path, site_path, field_set, type_set
- implement first_seen
- implement first_non_empty
- implement chunk-local priority sample candidates
- implement SQLite merge/prune for priority samples
```

Done criteria:

```text
- every materialized sample key has first_seen
- first_non_empty appears when non-empty candidate exists
- empty first_seen does not prevent later first_non_empty
- priority samples are bounded after flush
  - heterogeneous object array fixture produces multiple prof_shape rows under the same array site
- no prof_path_sample or prof_shape_sample table exists
  - no prof_array_* table exists
```

Default commit:

```text
feat(shape): collect shape facts and object samples
```

---

## Phase 6: Field and Value Accumulators

Goal:

```text
Build shape-specific field profiles and value sample scaffolding.
```

Work:

```text
- compute field_profile_id
- populate prof_shape_field
- track profiled_count, null_count, non_null_count
- track empty_object_count and empty_array_count
- compute value_hash without hot-path canonical JSON string materialization where practical
- prepare prof_field_value_sample rows
```

Done criteria:

```text
- null-only, empty-object-only, and empty-array-only facts are represented
- same field name across shape/type contexts gets separate field_profile_id
- value samples retain source-backed context
```

Default commit:

```text
feat(field): accumulate shape-specific field profiles
```

---

## Phase 7: Bounded Exact Distribution

Goal:

```text
Provide exact full distribution for small field profiles.
```

Work:

```text
- implement bounded exact counter
- enforce exact_distinct_threshold
- enforce per-field value byte budget
- enforce global exact value budget
- write exact_full rows when bounded
- fall back when threshold is exceeded
```

Done criteria:

```text
- small categorical fixture writes exact_full complete distribution
- threshold-crossing fixture does not write complete distribution
- fallback does not lose distinct count capability because HLL is updated from start
```

Default commit:

```text
feat(value): add bounded exact distributions
```

---

## Phase 8: Sketches and Priority Sampling

Goal:

```text
Implement bounded large-field profiling algorithms.
```

Work:

```text
- implement HyperLogLog
- implement Space-Saving
- implement deterministic priority sampling helpers
- update HLL, Space-Saving, and priority sampler from the start of each field profile
- write heavy hitter candidates
- write value priority samples
- do not write heavy_hitter_context rows by default
- if heavy_hitter_context is enabled later, write it only for final surviving heavy hitter values
```

Done criteria:

```text
- HLL estimate is written for large field profile
- heavy hitter candidates are bounded
- value priority samples are bounded
- heavy_hitter_context rows are 0 by default
- first_seen and first_non_empty value samples work
```

Default commit:

```text
feat(sketch): add approximate value profiling
```

---

## Phase 9: SQLite Writer Integration

Goal:

```text
Wire all accumulators into batched SQLite writes.
```

Work:

```text
- implement writer batches
- implement chunk flush
- implement priority sample pruning SQL
- create indexes after bulk insert where practical
- write prof_source_summary at end
```

Done criteria:

```text
- profile.sqlite is usable after successful run
- sample buffers are flushed and cleared
- priority samples remain within configured limits
- profile summary counts match table counts
```

Default commit:

```text
feat(sqlite): write profile facts in batches
```

---

## Phase 10: Stdout, Warnings, and Perf Log

Goal:

```text
Finalize user-visible command behavior.
```

Work:

```text
- print default stdout summary
- include elapsed time
- implement --quiet
- print warnings to stderr
- implement --perf-log timing/progress events
- implement --perf-log-file flushed file output
- implement --perf-log-dbstat optional diagnostics
```

Done criteria:

```text
- default stdout has only output path, summary counts, elapsed time
- --quiet suppresses normal stdout
- warnings do not stop usable output
- --perf-log does not pollute stdout
- --perf-log-file is flushed during execution
- --perf-log emits progress before command completion
```

Default commit:

```text
feat(cli): finalize output and perf log contract
```

---

## Phase 11: Fixtures and Regression Tests

Goal:

```text
Create regression fixtures and SQL assertions.
```

Work:

```text
- basic JSON fixture
- JSONL fixture
- heterogeneous shape fixture
- heterogeneous object array fixture
- empty first_seen then non-empty fixture
- exact distribution fixture
- approximate fallback fixture
- large finite JSONL smoke fixture
- SQL assertion scripts
```

Done criteria:

```text
- fixture tests pass
- no unexpected prof_* tables exist
- object sample coverage is verified for all four grains
- heterogeneous object array fixture verifies multiple prof_shape rows for one array site
- no prof_array_* tables are created
- exact/approx fallback is verified
```

Default commit:

```text
test(profile): add regression fixtures
```

---

## Phase 12: Performance Validation

Goal:

```text
Validate performance properties on large finite inputs.
```

Work:

```text
- run large JSONL benchmark
- capture --perf-log output
- check sample chunk flush behavior
- check profile.sqlite size
- check memory does not grow with sample key count except persisted bounded rows
```

Done criteria:

```text
- no full source materialization
- no unbounded per-key sample state
- large JSONL run completes within acceptable local target
- perf-log has expected buckets
```

Default commit:

```text
perf(profile): validate large input profiling
```

---

## Final Acceptance Criteria

v0.1.0 is complete when:

```text
1. CLI contract is implemented.
2. config YAML contract is implemented.
3. refs contract is validated.
4. profile.sqlite contains only approved physical tables.
5. no SQLite views are created.
6. JSON and JSONL file inputs work.
7. stdin is rejected.
8. shape-aware value profiles are populated.
9. object samples cover canonical/site/field_set/type_set grains.
10. first_seen is guaranteed per materialized sample key.
11. first_non_empty is captured when available.
12. object and value priority samples are chunk-flushed and bounded.
13. heterogeneous object arrays are represented through existing prof_shape rows.
14. no prof_array_* tables are created.
15. exact full distribution is used for bounded small field profiles.
16. HLL and heavy hitter fallback works for larger profiles.
17. default stdout matches the summary-only contract.
18. warnings are stderr-only and non-fatal when output is usable.
19. --perf-log emits incremental timing/progress events and final buckets.
20. heavy_hitter_context rows are 0 by default.
21. safer value context defaults are documented and implemented.
22. regression fixtures pass.
```
