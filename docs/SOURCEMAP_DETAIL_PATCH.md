# SOURCEMAP additions for split implementation detail docs

Add or update the `docs/plans` section as follows:

```text
docs/plans/PROFILE_IMPLEMENTATION_DETAIL_PLAN.md
  Index for split code-level implementation detail documents; maps master phases to implementation detail files.

docs/plans/detail/PROFILE_DETAIL_01_CLI_CONFIG.md
  CLI parser, YAML config, precedence, validation, and file-only input policy.

docs/plans/detail/PROFILE_DETAIL_02_SQLITE_REFS.md
  prof_* DDL, approved/forbidden schema objects, refs DB contract validation, refs resolver.

docs/plans/detail/PROFILE_DETAIL_03_SCAN_SHAPE_SAMPLE.md
  JSON/JSONL scanner, source paths, shape identity, object sampling, heterogeneous object arrays.

docs/plans/detail/PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md
  field_profile_id, field summaries, empty_string_count, value identity, exact counters, HLL, Space-Saving, value samples.

docs/plans/detail/PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md
  SQLite writer, chunk flush, sample prune SQL, stdout/stderr, perf-log, performance invariants.

docs/plans/detail/PROFILE_DETAIL_06_FIXTURES_TESTS.md
  Fixture matrix, SQL assertions, forbidden object checks, output contract tests, performance smoke.
```


Additional rc.2 mapping:

```text
docs/SPEC.md
  v0.1.0-rc.2 candidate status, perf-log options, heavy_hitter_context default-off policy.

docs/CLI_CONTRACT.md
  --perf-log-file, --perf-log-dbstat, and safer value-sampling defaults.

docs/PERFORMANCE.md
  Incremental perf-log requirements and large-input value sample safety.

docs/PROFILE_JSON_REFS_PERF_DIAGNOSIS.md
  Accepted diagnosis that motivated the rc.2 performance-safe candidate.
```
