# Performance Requirements

Release status: `v0.1.0-rc.2`.

rc.2 incorporates the large-input value-sampling fix: heavy hitter context samples are disabled by default, value context payloads are smaller by default, and `--perf-log` must provide useful progress before process completion.

`profile-json-refs` targets large finite JSON/JSONL inputs.

It does not claim constant-memory processing for infinite streams, but it must avoid avoidable hot-path materialization and unbounded sample memory growth.

---

## 1. Required Properties

```text
- streaming source scan
- no full source materialization
- chunk-flushed object samples
- chunk-flushed value samples
- bounded exact value counters
- bounded heavy hitter state
- bounded HLL state
- bounded sample body storage
- deferred canonical JSON / value_text materialization
- batched SQLite writes
- optional --perf-log timing buckets to stderr
```

---

## 2. Hot Path Materialization Policy

Semantic value identity is:

```text
stable_hash(canonical_json(value))
```

Implementation must not repeatedly materialize canonical JSON strings for every value on the hot path.

Preferred flow:

```text
observed value
  -> typed token / compact identity / interned value representation
  -> stable value hash
  -> materialize value_text only if selected for storage
```

This mirrors the upstream performance lesson that repeated canonical string construction can dominate large JSONL workloads.

---

## 3. Shape Identity Policy

Shape identity should also avoid repeated string-heavy keys where possible.

Preferred flow:

```text
object fields
  -> interned field/type tokens
  -> field_set_hash and type_set_hash
  -> shape_id
  -> materialize field_set_json/type_set_json at flush boundary
```

Do not use hash-only identity if collisions would merge distinct shapes. Use collision-safe token equality or materialized equality checks where needed.

---

## 4. Heterogeneous Object Array Cardinality

Heterogeneous object arrays may increase `prof_shape`, `prof_shape_field`, and `prof_object_sample` cardinality.

v0.1.0 handles heterogeneous object arrays through existing shape profiling:

```text
array object element
  -> resolved canonical_path / site_path / schema_path
  -> field_set_hash / type_set_hash
  -> prof_shape
```

The implementation must not add unbounded array-specific accumulator state in v0.1.0.

Dedicated array statistics are deferred. No `prof_array_*` tables are created in v0.1.0.

---

## 5. Sample OOM Risk

Samples can cause OOM when key cardinality is high.

Risky pattern:

```text
keep one long-lived sampler per sample key in memory
```

Required pattern:

```text
first_seen:
  insert immediately or at chunk flush with unique semantics

first_non_empty:
  insert immediately or at chunk flush with unique semantics

priority_sample:
  keep only chunk-local top-K candidates
  merge into SQLite at chunk flush
  prune persisted rows to configured per-key limits
```

Chunk flush is required for sample safety.

### 5.1 Value Context Safety

`v0.1.0-rc.2` treats `heavy_hitter_context` as optional and disabled by default.

```text
sampling.value.heavy_hitter_context_sample_limit = 0
```

The scanner must not write heavy hitter context rows for transient Space-Saving candidates. High-cardinality fields must not create value-context rows proportional to distinct value count.

If enabled, heavy hitter context rows are finalization-only and limited to final surviving heavy hitter values.

Default value context payload limits are intentionally small:

```text
sampling.value.value_json_limit_bytes = 1024
sampling.value.parent_object_json_limit_bytes = 1024
sampling.value.priority_sample_limit_per_field_profile = 4
```

---

## 6. Chunk Flush

Recommended flush triggers:

```text
- object sample row buffer reaches sampling.object.chunk_flush_rows
- value sample row buffer reaches sampling.value.chunk_flush_rows
- SQLite writer batch reaches configured row count
- memory pressure guard triggers
- end of input
```

At flush:

```text
1. insert first_seen / first_non_empty rows with ignore-on-conflict semantics
2. insert priority sample candidates
3. prune priority samples per key
4. write aggregate rows when appropriate
5. clear chunk-local sample buffers
```

---

## 7. Exact Counter Budget

Exact full distribution is preferred for small field profiles, but it must be bounded.

Recommended defaults:

```text
exact_distinct_threshold: 4096
exact_value_bytes_per_field_profile: 1048576
global_exact_value_bytes_budget: 268435456
```

When thresholds are exceeded, the field profile must fall back to HLL + heavy hitters + samples.

---

## 8. SQLite Write Policy

```text
- create tables before scan
- write sample rows in chunks
- write aggregate rows in batches
- reuse prepared statements where practical
- create indexes after large insert phases when possible
- close database cleanly
```

`profile.sqlite` should be either a usable artifact or the command should fail clearly.

---

## 9. Performance Log

`v0.1.0-rc.2` strengthens `--perf-log`.

`--perf-log` must emit useful progress before process completion. Final-only perf output is insufficient for large JSONL runs.

Destinations:

```text
--perf-log:
  write perf events to stderr

--perf-log-file <FILE>:
  write perf events to FILE and flush during execution

--perf-log-dbstat:
  include optional SQLite dbstat size diagnostics
```

Minimum progress events:

```text
scan.progress:
  documents, objects, arrays, scalars, source bytes or line number when available

flush.chunk:
  chunk index, object sample rows, value sample rows, shape rows, field rows

sqlite.prune_samples:
  prune kind, touched keys, rows before/after when practical, elapsed

sqlite.rows:
  prof_shape, prof_shape_field, prof_object_sample, prof_field_value_sample,
  prof_field_summary, prof_field_value

sqlite.size:
  profile.sqlite, profile.sqlite-wal, profile.sqlite-shm bytes
```

Suggested final buckets:

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

Example progress output:

```text
[perf] t=31.882s phase=scan.progress documents=100000 objects=432901 arrays=1022 scalars=981223
[perf] t=35.104s phase=flush.chunk index=12 shapes=41 fields=320 object_samples=200 value_samples=10000
[perf] t=35.891s phase=sqlite.prune_samples kind=value_priority touched_fields=831 rows_before=48210 rows_after=32000 elapsed=0.402s
[perf] t=36.002s phase=sqlite.size db=214958080 wal=12582912 shm=163840
```

When enabled, perf logging must be flushed periodically so `perf.log` can diagnose a long-running process while it is still running.

---

## 10. Source Layout Notes

Performance-relevant modules:

```text
src/value/identity.rs
src/value/interner.rs
src/value/exact_counter.rs
src/shape/token.rs
src/shape/sample.rs
src/sketch/hll.rs
src/sketch/space_saving.rs
src/sketch/priority_sample.rs
src/perf/timer.rs
src/sqlite/writer.rs
```

`src/sketch/reservoir.rs` is not part of the v0.1.0 plan. Object/value samples use chunk-mergeable deterministic priority sampling.

---

## 11. Acceptance Criteria

```text
- no full input materialization
- no unbounded per-key sample state
- first_seen samples exist for every materialized sample key
- first_non_empty is captured when available
- priority samples are bounded after every flush
- exact counters fall back when thresholds are exceeded
- --perf-log emits incremental timing/progress events and final buckets
- default stdout remains summary-only
```
