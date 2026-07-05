# CLI Contract

This document defines the `v0.1.0-rc.2` command-line contract for `profile-json-refs`. If rc.2 is accepted, this becomes the final `v0.1.0` CLI contract.

`profile-json-refs` reads one JSON/JSONL source file and `refs/schemas.sqlite`, then writes `profile.sqlite`.

The primary output is `profile.sqlite`. Stdout is only a short summary.

---

## 1. Command Shape

Default form:

```bash
profile-json-refs <INPUT_FILE>
```

Expanded form:

```bash
profile-json-refs <INPUT_FILE> --refs refs/schemas.sqlite --out profile.sqlite
```

JSONL example:

```bash
dump-json-refs data.jsonl --jsonl --outdir refs
profile-json-refs data.jsonl --jsonl
```

---

## 2. Input Policy

`profile-json-refs` v0.1.0 requires a filesystem input path.

Supported:

```bash
profile-json-refs data.json
profile-json-refs data.jsonl --jsonl
```

Not supported in v0.1.0:

```bash
cat data.jsonl | profile-json-refs --jsonl
profile-json-refs - --jsonl
```

Rationale: `profile-json-refs` profiles one source snapshot against `refs/schemas.sqlite`. A named source path keeps the relationship between the source file, refs database, and generated profile artifact explicit.

---

## 3. Options

```text
Arguments:
  <INPUT_FILE>
      Input JSON/JSONL file.
      Stdin is not supported in v0.1.0.

Options:
  --refs <FILE>
      Path to refs/schemas.sqlite.
      Default: refs/schemas.sqlite

  -o, --out <FILE>
      Output SQLite database path.
      Default: profile.sqlite

  --jsonl
      Force JSONL input mode.

  --config <FILE>
      Read configuration from YAML.

  --shape-sample-limit <N>
      Convenience override for sampling.object.type_set.priority_sample_limit.

  --value-sample-limit <N>
      Convenience override for sampling.value.priority_sample_limit_per_field_profile.

  --heavy-hitter-limit <N>
      Maximum stored heavy hitter candidates per field profile.

  --hll-precision <N>
      HyperLogLog precision parameter.

  --value-text-limit <BYTES>
      Maximum stored value_text bytes before truncation.

  --perf-log
      Print detailed timing and progress events. Default destination: stderr.

  --perf-log-file <FILE>
      Write perf-log events to a file instead of stderr. The file is flushed during execution.

  --perf-log-dbstat
      Include optional SQLite dbstat diagnostics in perf-log output. This may be expensive.

  --quiet
      Suppress normal stdout summary.

  --help
      Print help.
```

`--strict` is not part of the v0.1.0 contract.

---

## 4. Defaults

```text
refs path:
  refs/schemas.sqlite

output path:
  profile.sqlite

input mode:
  json unless --jsonl is set or implementation-supported file extension detection selects jsonl

stdout:
  output path + prof_source_summary-level counts + elapsed time

stderr:
  warnings, errors, optional --perf-log output unless --perf-log-file is used

rc.2 value sampling defaults:
  value_json_limit_bytes = 1024
  parent_object_json_limit_bytes = 1024
  priority_sample_limit_per_field_profile = 4
  heavy_hitter_context_sample_limit = 0

rc.2 value text default:
  value_text_limit_bytes = 512
```

---

## 5. Configuration YAML

`--config <FILE>` reads execution settings from YAML.

The config file is not stored in `profile.sqlite`. `profile.sqlite` stores profile facts only.

### 5.1 Precedence

When the same option is provided in multiple places, precedence is:

```text
1. explicit CLI option
2. config YAML
3. built-in default
```

Example:

```bash
profile-json-refs data.jsonl --jsonl --config profile.yaml --heavy-hitter-limit 256
```

If `profile.yaml` sets `value_profile.heavy_hitter_limit: 128`, the effective value is `256`.

### 5.2 Minimal config

```yaml
refs:
  sqlite: refs/schemas.sqlite

output:
  sqlite: profile.sqlite

value_profile:
  hll_precision: 14
  heavy_hitter_limit: 128
  value_text_limit_bytes: 512
```

### 5.3 Recommended config

```yaml
# profile-json-refs config.yaml

input:
  # Optional. CLI <INPUT_FILE> is preferred for normal use.
  # If both are provided, the CLI argument wins.
  file: null
  format: auto        # auto | json | jsonl

refs:
  sqlite: refs/schemas.sqlite

output:
  sqlite: profile.sqlite

stdout:
  quiet: false

perf:
  log: false          # same behavior as --perf-log
  file: null          # same behavior as --perf-log-file
  dbstat: false       # same behavior as --perf-log-dbstat

sampling:
  object:
    # Maximum stored bytes for object-level sample_json before truncation.
    sample_json_limit_bytes: 16384

    # Number of sample rows buffered before chunk merge/prune.
    chunk_flush_rows: 10000

    canonical_path:
      first_seen: true
      first_non_empty: true
      priority_sample_limit: 1

    site_path:
      first_seen: true
      first_non_empty: true
      priority_sample_limit: 1

    field_set:
      first_seen: true
      first_non_empty: true
      priority_sample_limit: 2

    type_set:
      first_seen: true
      first_non_empty: true
      priority_sample_limit: 4

  value:
    value_json_limit_bytes: 1024
    parent_object_json_limit_bytes: 1024
    chunk_flush_rows: 10000

    first_seen: true
    first_non_empty: true
    priority_sample_limit_per_field_profile: 4
    heavy_hitter_context_sample_limit: 0

value_profile:
  value_text_limit_bytes: 512

  exact_distinct_threshold: 4096
  exact_value_bytes_per_field_profile: 1048576
  global_exact_value_bytes_budget: 268435456

  hll_precision: 14
  heavy_hitter_limit: 128
```

### 5.4 Full structure

```text
input.file
input.format
refs.sqlite
output.sqlite
stdout.quiet
perf.log
sampling.object.sample_json_limit_bytes
sampling.object.chunk_flush_rows
sampling.object.canonical_path.first_seen
sampling.object.canonical_path.first_non_empty
sampling.object.canonical_path.priority_sample_limit
sampling.object.site_path.first_seen
sampling.object.site_path.first_non_empty
sampling.object.site_path.priority_sample_limit
sampling.object.field_set.first_seen
sampling.object.field_set.first_non_empty
sampling.object.field_set.priority_sample_limit
sampling.object.type_set.first_seen
sampling.object.type_set.first_non_empty
sampling.object.type_set.priority_sample_limit
sampling.value.value_json_limit_bytes
sampling.value.parent_object_json_limit_bytes
sampling.value.chunk_flush_rows
sampling.value.first_seen
sampling.value.first_non_empty
sampling.value.priority_sample_limit_per_field_profile
sampling.value.heavy_hitter_context_sample_limit
value_profile.value_text_limit_bytes
value_profile.exact_distinct_threshold
value_profile.exact_value_bytes_per_field_profile
value_profile.global_exact_value_bytes_budget
value_profile.hll_precision
value_profile.heavy_hitter_limit
```

### 5.5 CLI-to-YAML mapping

| CLI option | YAML key |
|---|---|
| `<INPUT_FILE>` | `input.file` |
| `--jsonl` | `input.format = jsonl` |
| `--refs <FILE>` | `refs.sqlite` |
| `--out <FILE>` | `output.sqlite` |
| `--quiet` | `stdout.quiet = true` |
| `--perf-log` | `perf.log = true` |
| `--perf-log-file <FILE>` | `perf.file` |
| `--perf-log-dbstat` | `perf.dbstat = true` |
| `--shape-sample-limit <N>` | `sampling.object.type_set.priority_sample_limit` |
| `--value-sample-limit <N>` | `sampling.value.priority_sample_limit_per_field_profile` |
| `--heavy-hitter-limit <N>` | `value_profile.heavy_hitter_limit` |
| `--hll-precision <N>` | `value_profile.hll_precision` |
| `--value-text-limit <BYTES>` | `value_profile.value_text_limit_bytes` |

`--shape-sample-limit` is a convenience option for the most detailed object sample grain: `type_set`. Use config YAML to tune canonical/site/field-set limits independently.

### 5.6 Validation

Invalid config should fail before scanning starts.

Examples of invalid config:

```text
- unknown top-level section
- negative limit
- zero hll_precision
- heavy_hitter_context_sample_limit must be >= 0; 0 disables heavy hitter context samples
- unsupported input.format
- exact_distinct_threshold < heavy_hitter_limit is allowed but should warn if it makes exact fallback ineffective
- missing input file after CLI/config resolution
```

Unknown YAML keys should be rejected in v0.1.0. Silent ignore makes configuration errors hard to diagnose.

---

## 6. Stdout Contract

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

Only the output path, summary counts, and elapsed time are printed.

No detailed shape, field, value, or sample rows are printed to stdout.

---

## 7. Quiet Mode

```bash
profile-json-refs data.jsonl --jsonl --quiet
```

Behavior:

```text
stdout:
  no output on success

stderr:
  warnings, errors, and --perf-log output only
```

`--quiet` suppresses normal stdout only. It does not suppress warnings, errors, or requested performance logs.

---

## 8. Performance Log

`--perf-log` prints detailed timing buckets to stderr.

Example:

```bash
profile-json-refs data.jsonl --jsonl --perf-log 2> perf.log
```

Example stderr:

```text
[perf] total=41.238s
[perf] refs.open=0.012s
[perf] refs.load_contract=0.038s
[perf] scan.read_parse_walk=31.842s
[perf] flush.object_samples=0.820s
[perf] flush.shapes=1.104s
[perf] flush.fields=2.881s
[perf] flush.values=3.916s
[perf] sqlite.indexes=1.041s
[perf] t=12.345 phase=scan.accumulators pending_shapes=120 pending_shape_fields=940 pending_object_samples=320 pending_value_samples=10000 field_value_accumulators=940
[perf] t=12.456 phase=sqlite.flush.value_samples elapsed_ms=58 rows=10000
[perf] t=12.470 phase=sqlite.prune.value_priority elapsed_ms=14 fields=87
[perf] t=41.200 phase=sqlite.summary.counts elapsed_ms=3 canonical_paths=42 site_paths=42 shapes=120 field_profiles=940 stored_values=4000
[perf] t=41.220 phase=sqlite.size profile_sqlite_bytes=12345678 profile_sqlite_wal_bytes=0 profile_sqlite_shm_bytes=0
```

---

## 9. Warnings

Warnings are written to stderr and do not stop execution when a usable `profile.sqlite` can still be written.

Example:

```text
warning: W_VALUE_TEXT_TRUNCATED: some values exceeded --value-text-limit
warning: W_REFS_SOURCE_MISMATCH: refs/source counts differ
warning: W_OBJECT_SAMPLE_LIMIT_REACHED: some sample keys reached priority_sample_limit
```

Warnings are not stored in `profile.sqlite`.

---

## 10. Exit Behavior

```text
success:
  profile.sqlite written
  exit 0

usable partial result:
  profile.sqlite written
  warnings printed to stderr
  exit 0

fatal failure:
  profile.sqlite could not be written
  error printed to stderr
  non-zero exit
```
