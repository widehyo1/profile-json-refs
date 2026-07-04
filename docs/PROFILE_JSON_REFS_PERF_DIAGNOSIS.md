# profile-json-refs Large JSONL Performance Diagnosis

Release status: this diagnosis has been accepted into `v0.1.0-rc.2`. The rc.2 candidate treats the performance fix, safer defaults, and strengthened perf logging as part of the final `v0.1.0` release-candidate contract.

This document summarizes the observed long-running `profile-json-refs` execution, the diagnostic evidence, its meaning, the likely root causes, required fixes, affected code/documentation targets, and proposed `--perf-log` improvements.

Context:

```text
tool: profile-json-refs
input: claude_merged.jsonl
input size: 2.9G
refs size: 334M
profile.sqlite size during run: 1.2G
profile.sqlite-wal size during run: 66M
runtime at diagnosis: ~140 minutes
process CPU: ~98% single core
```

Observed working directory snapshot:

```text
2.9G    claude_merged.jsonl
0       perf.log
1.2G    profile.sqlite
160K    profile.sqlite-shm
66M     profile.sqlite-wal
334M    refs
26M     temp.json
```

---

## 1. Executive Summary

The current run is unlikely to be caused by a literal infinite loop.

The stronger diagnosis is:

```text
bounded work is being repeated too expensively on large data,
especially around prof_field_value_sample writes and pruning.
```

The decisive evidence is that nearly all SQLite size is concentrated in:

```text
prof_field_value_sample
```

Observed size:

```text
prof_field_value_sample: 1067.5 MB
```

At the same time:

```text
prof_field_summary: 0 rows
prof_field_value:   0 rows
prof_source_summary: 0 MB / not finalized
```

This means the run had not reached final value-summary/value-distribution flush. It was still spending time in scan/chunk write/sample write/prune work.

The largest problematic sample kind is:

```text
heavy_hitter_context: 306,571 rows
```

This indicates that `heavy_hitter_context` is acting like a high-cardinality value-context sample table, not as context for final heavy hitter values.

Final diagnosis:

```text
Not an infinite loop.
Not primarily heterogeneous object array shape explosion.
Primary issue: value sample explosion, especially heavy_hitter_context.
Secondary issue: expensive SQLite table-wide prune/update and large parent_object_json storage.
```

---

## 2. Observed Runtime Evidence

### 2.1 Process I/O

Sample from `/proc/$pid/io`:

```text
Sat Jul  4 19:27:51 KST 2026
rchar: 388637600867
wchar: 49358115212
read_bytes: 0
write_bytes: 31234007040
cancelled_write_bytes: 0

Sat Jul  4 19:28:01 KST 2026
rchar: 390661801475
wchar: 49579564372
read_bytes: 0
write_bytes: 31390527488
cancelled_write_bytes: 0

Sat Jul  4 19:28:11 KST 2026
rchar: 390699083267
wchar: 49589272164
read_bytes: 0
write_bytes: 31391277056
cancelled_write_bytes: 0
```

Meaning:

```text
rchar is far larger than the 2.9GB input file.
This indicates repeated logical reads from SQLite pages, indexes, WAL, or page cache,
not just a single pass over the source file.

read_bytes = 0 means reads are likely served from page cache.
This does not mean no work is happening.

write_bytes continues to increase, so SQLite is still writing pages.
```

### 2.2 Open Files

Relevant `lsof` output:

```text
fd 3ur  refs/schemas.sqlite
fd 7ur  profile.sqlite
fd 8u   profile.sqlite-wal
fd 9uw  profile.sqlite-shm
fd 10r  claude_merged.jsonl
fd 2w   perf.log
```

Meaning:

```text
The process still has the input file open, but strace shows heavy activity on
profile.sqlite and profile.sqlite-wal rather than primarily reading the input.
```

### 2.3 strace

Sample:

```text
pwrite64(8, ..., 4096, ...)
pread64(8, ..., 4096, ...)
pread64(7, ..., 4096, ...)
pwrite64(8, ..., 24, ...)
pwrite64(8, ..., 4096, ...)
```

Where:

```text
fd 7 = profile.sqlite
fd 8 = profile.sqlite-wal
```

Meaning:

```text
The hot activity is SQLite profile DB/WAL read/write.
This is consistent with repeated insert/prune/update activity on already-large tables.
```

---

## 3. SQLite State During Run

### 3.1 Row Counts

Observed:

```text
table_name               rows
-----------------------  ------
prof_shape               1458
prof_shape_field         8746
prof_object_sample       16819
prof_field_summary       0
prof_field_value         0
prof_field_value_sample  376525
```

Meaning:

```text
prof_shape and prof_shape_field are small.
Shape explosion is not the main cause.

prof_field_summary and prof_field_value are still empty.
The run has not reached final value-summary/value-distribution output.

prof_field_value_sample already has 376,525 rows.
This table is the active growth and performance risk.
```

### 3.2 Sample Kind Counts

Object samples:

```text
sample_kind      rows
---------------  ----
priority_sample  9319
first_seen       3755
first_non_empty  3745
```

Value samples:

```text
sample_kind           rows
--------------------  ------
heavy_hitter_context  306546
priority_sample       52736
first_seen            8746
first_non_empty       8497
```

Meaning:

```text
Object samples are not the main problem.

Value samples dominate, especially heavy_hitter_context.
heavy_hitter_context alone accounts for most rows.
```

### 3.3 SQLite Object Sizes

Observed via `dbstat`:

```text
name                                        mb
------------------------------------------  ------
prof_field_value_sample                     1067.5
prof_object_sample                          33.67
idx_prof_field_value_sample_field           11.07
idx_prof_field_value_sample_hash            10.5
sqlite_autoindex_prof_field_value_sample_1  9.81
idx_prof_object_sample_key                  2.86
idx_prof_object_sample_once                 1.23
prof_shape                                  1.09
prof_shape_field                            0.81
...
prof_field_summary                          0.0
prof_field_value                            0.0
```

Meaning:

```text
The profile.sqlite size is overwhelmingly caused by prof_field_value_sample.

Indexes are not the biggest storage object, but maintaining them during insert/prune
adds write and CPU overhead.

The final distribution tables have not yet been populated.
```

---

## 4. Parent JSON Size Analysis

Observed query result:

```text
sample_kind           rows    value_json_mb  parent_json_mb  avg_parent_json  max_parent_json
--------------------  ------  -------------  --------------  ---------------  ---------------
heavy_hitter_context  306571  105.92         530.41          1814.2           16384
priority_sample       52847   5.52           97.37           1931.9           16384
first_seen            8746    0.88           18.04           2162.8           16384
first_non_empty       8497    0.89           17.96           2216.9           16384
```

Meaning:

```text
Average parent_object_json size is around 1.8KB to 2.2KB.
This is not absurdly large per row.

The problem is row count, especially heavy_hitter_context with 306k rows.

heavy_hitter_context alone stores approximately:
  value_json + parent_object_json ~= 636MB

After SQLite row overhead, B-tree pages, indexes, and fragmentation,
prof_field_value_sample reaching ~1GB is expected.
```

---

## 5. Heavy Hitter Context Analysis

Observed top field profiles:

```text
field_profile_id   rows  distinct_value_hashes
-----------------  ----  ---------------------
7fd5b473b15fbc6f   9416  9296
c4161b28d370be4f   7229  7103
f1b474def0011697   7184  7059
e96b97d4178341d6   6919  6797
7752f800283ad335   6702  6580
f2a16cf5ee60c5e7   6681  6560
320e2c730abc0d9f   6672  6550
bda947a5f1a0ff93   6304  6185
958fa8c3ab184929   6293  6173
a4ef6b574627cda3   6155  6029
```

Meaning:

```text
rows ~= distinct_value_hashes.

This is not heavy hitter behavior.
This means nearly every distinct value in those field profiles gets context.

Expected heavy hitter context should look more like:
  rows <= heavy_hitter_limit * heavy_hitter_context_sample_limit

For default heavy_hitter_limit = 128 and context limit = 1:
  rows per field_profile_id should be <= 128

Observed rows per field_profile_id are thousands.
```

Therefore:

```text
heavy_hitter_context is currently behaving like a high-cardinality value-context sample table.
```

### 5.1 Duplicate Check

Observed duplicates:

```text
field_profile_id   value_hash          rows
-----------------  ------------------  ----
009e1e7bfa5ab849   6d659da9703576b1   2
009e1e7bfa5ab849   7daba9bf6eae1997   2
...
02842831827890e5   76a2d46f3ffacd2e   2
```

Meaning:

```text
The main problem is not unlimited duplicate context for the same value_hash.
Most duplicates are small, often 2.

The main problem is that thousands of distinct value_hashes get heavy_hitter_context.
```

---

## 6. Root Cause Ranking

### Root Cause 1: heavy_hitter_context is emitted during scan for too many candidate values

Current behavior appears to be:

```text
observe value
  -> update Space-Saving candidate state
  -> emit or retain heavy_hitter_context for many observed candidates
  -> write context sample rows to SQLite during chunk flush
```

This is wrong for high-cardinality fields.

Correct behavior:

```text
scan:
  update Space-Saving only
  do not write heavy_hitter_context rows

finalization:
  compute final surviving heavy hitter candidates
  optionally write context only for those final candidates
```

Recommended v0.1.0 policy:

```text
heavy_hitter_context is disabled by default.
heavy_hitter_context_sample_limit = 0
```

### Root Cause 2: parent_object_json is too expensive as a default sample payload

Current default appears effectively large:

```text
parent_object_json_limit_bytes = 16384
```

Observed max:

```text
16384
```

This allows many rows to carry up to 16KiB context.

Recommended default:

```text
parent_object_json_limit_bytes = 1024
```

For heavy hitter context:

```text
do not store parent_object_json by default
```

### Root Cause 3: sample materialization likely happens before top-K admission

If code builds full sample rows before checking priority top-K, it serializes and truncates:

```text
value_json
parent_object_json
object sample JSON
```

even for samples that will be discarded.

Correct behavior:

```text
compute deterministic priority first
check top-K admission
materialize JSON only if admitted
```

### Root Cause 4: prune queries likely operate on whole sample tables repeatedly

If every chunk flush runs window-function prune over all existing sample rows, cost increases with table size.

Problematic pattern:

```text
flush 1 scans/prunes 10k rows
flush 2 scans/prunes 20k rows
flush 3 scans/prunes 30k rows
...
```

Total cost trends toward:

```text
O(total_rows * flush_count)
```

Correct behavior:

```text
collect touched sample keys in each chunk
prune only touched sample keys
```

### Root Cause 5: final value tables are not yet populated, so all observed cost is pre-finalization overhead

Because:

```text
prof_field_summary = 0
prof_field_value = 0
```

the run is stuck before final summary/distribution output. The sample subsystem is expensive enough to dominate execution before the final profile facts are written.

### Root Cause 6: global exact budget may not be enforced

If `global_exact_value_bytes_budget` exists in config/spec but is not actually enforced in code, final `prof_field_value` can grow substantially after the current sample bottleneck is fixed.

This did not cause the current 1GB table, because `prof_field_value` is still empty, but it is a likely next bottleneck.

---

## 7. Interpretation: JSONL property vs Implementation Issue

The input being a 2.9GB JSONL file can legitimately produce:

```text
- many objects
- many scalar values
- high-cardinality fields
- many field_profile_id observations
- large value sample pressure
```

However, the observed profile state indicates the implementation amplifies that pressure incorrectly.

Key point:

```text
prof_shape = 1458
prof_shape_field = 8746
```

These are not extremely large.

Therefore the dominant issue is not:

```text
heterogeneous object array shape explosion
```

The dominant issue is:

```text
value sample policy and implementation, especially heavy_hitter_context
```

---

## 8. Immediate Operational Recommendation

Stop the current run.

Reason:

```text
- profile.sqlite is already dominated by an oversized sample table.
- final summary/value tables are still empty.
- waiting may produce even more SQLite write/prune work.
- perf.log remains empty until process completion, so this run gives limited timing insight.
```

Suggested commands:

```bash
kill 78408
sleep 5
kill -KILL 78408 2>/dev/null || true

rm -f profile.sqlite profile.sqlite-wal profile.sqlite-shm perf.log
```

Before deleting, keep a copy if further forensic queries are needed.

---

## 9. Required Fixes

### Fix 1: Disable heavy_hitter_context by default

Change default:

```text
heavy_hitter_context_sample_limit: 1
```

to:

```text
heavy_hitter_context_sample_limit: 0
```

Validation must allow `0`.

### Fix 2: Do not emit heavy_hitter_context during scan

The scanner/value accumulator must not write `heavy_hitter_context` rows for observed Space-Saving candidates.

Required policy:

```text
heavy_hitter_context is finalization-only.
```

For v0.1.0, simpler policy:

```text
heavy_hitter_context is disabled by default and may remain unimplemented unless explicitly enabled later.
```

### Fix 3: If enabled, only final surviving heavy hitter values may get context

Allowed finalization behavior:

```text
for each field_profile_id:
  final_top = space_saving.top().take(heavy_hitter_limit)

  for value in final_top:
    write prof_field_value heavy_hitter row

    if heavy_hitter_context_sample_limit > 0:
      write at most K context samples for this final value
```

Not allowed:

```text
write heavy_hitter_context for every observed candidate during scan
```

### Fix 4: Lower parent_object_json default size

Recommended default:

```text
parent_object_json_limit_bytes: 1024
```

Optional stricter default:

```text
parent_object_json_limit_bytes: 512
```

Keep `object_json_limit_bytes` at 16KiB if needed for object navigation samples, but value context should be much smaller.

### Fix 5: Materialize sample JSON only after top-K admission

For priority samples:

```text
priority = stable_hash(sample identity)

if top_k.should_accept(priority):
  materialize value_json / parent_object_json / sample_json
  insert candidate
else:
  do not serialize JSON
```

This applies to:

```text
- object priority samples
- value priority samples
- any future heavy hitter context samples
```

### Fix 6: Prune only touched keys

During chunk accumulation, track:

```text
object samples:
  touched (sample_scope, sample_key)

value priority samples:
  touched field_profile_id

heavy hitter context samples:
  touched (field_profile_id, value_hash)
```

Prune SQL should be scoped to those touched keys only.

### Fix 7: Enforce global exact value budget

`global_exact_value_bytes_budget` must be connected to actual exact counter allocation.

Expected behavior:

```text
- exact counter remains enabled while per-field and global budgets allow it
- after budget pressure, exact_full is disabled for affected field profiles
- HLL and Space-Saving continue from the start
```

### Fix 8: Consider creating non-essential indexes after bulk insert

Keep only indexes required for correctness during writes:

```text
- primary keys
- unique index for first_seen / first_non_empty if needed
```

Move lookup/report indexes to finalization when practical.

---

## 10. Code Modification Targets

Likely files/modules to change:

```text
src/config.rs
  - default heavy_hitter_context_sample_limit = 0
  - default parent_object_json_limit_bytes = 1024
  - allow heavy_hitter_context_sample_limit = 0

src/value/sample.rs
  - remove scan-time heavy_hitter_context row generation
  - make sample row materialization lazy after top-K admission
  - treat "" as first_non_empty
  - avoid parent_object_json unless sample is accepted

src/field/accumulator.rs
  - update Space-Saving during scan
  - do not request heavy_hitter_context sample during scan
  - finalization-only heavy hitter rows

src/sketch/priority.rs
  - add should_accept(priority)
  - support lazy candidate materialization

src/sqlite/writer.rs
  - prune only touched keys
  - remove whole-table prune per chunk
  - optionally skip heavy_hitter_context prune when disabled
  - batch writes with prepared statements

src/value/exact_counter.rs
  - expose actual memory usage
  - integrate global exact budget

src/lib.rs
  - wire finalization order
  - ensure prof_field_summary/prof_field_value are written after scan
  - perf-log phase boundaries

src/perf/timer.rs
  - support incremental perf events and counters
```

Potential tests:

```text
tests/value_samples.rs
  - heavy_hitter_context disabled by default
  - no heavy_hitter_context rows when limit = 0
  - priority sample materialization is bounded
  - "" is first_non_empty

tests/sketches.rs
  - heavy hitter rows are bounded by heavy_hitter_limit
  - high-cardinality fields do not produce thousands of heavy_hitter_context rows

tests/sqlite_writer.rs
  - prune touched keys only
  - sample rows remain bounded after multiple chunk flushes

tests/perf_smoke.rs
  - large JSONL run does not produce oversized prof_field_value_sample
```

---

## 11. Documentation Modification Targets

Update these documents:

```text
docs/SPEC.md
docs/CLI_CONTRACT.md
docs/DATA_MODEL.md
docs/POPULATION_RULES.md
docs/PROBABILISTIC_DS.md
docs/PERFORMANCE.md
docs/SOURCEMAP.md

docs/plans/PROFILE_IMPLEMENTATION_DETAIL_PLAN.md
docs/plans/detail/PROFILE_DETAIL_01_CLI_CONFIG.md
docs/plans/detail/PROFILE_DETAIL_04_FIELD_VALUE_SKETCH.md
docs/plans/detail/PROFILE_DETAIL_05_WRITER_OUTPUT_PERF.md
docs/plans/detail/PROFILE_DETAIL_06_FIXTURES_TESTS.md
```

Recommended spec policy text:

```md
`heavy_hitter_context` is optional and disabled by default in v0.1.0.

The scanner must not emit `heavy_hitter_context` rows for every observed
Space-Saving candidate.

When enabled, heavy hitter context samples may only be written for final
surviving heavy hitter values after Space-Saving finalization.

This prevents high-cardinality fields from turning heavy hitter context into an
unbounded value-context sample table.
```

Recommended config default text:

```yaml
sampling:
  value:
    value_json_limit_bytes: 1024
    parent_object_json_limit_bytes: 1024
    priority_sample_limit_per_field_profile: 4
    heavy_hitter_context_sample_limit: 0
```

Recommended data model note:

```md
`prof_field_value_sample.sample_kind = 'heavy_hitter_context'` is optional.
When present, rows must correspond only to final surviving heavy hitter values,
not transient scan-time candidates.
```

Recommended performance note:

```md
High-cardinality fields must not create heavy hitter context rows proportional
to distinct value count.
```

---

## 12. Proposed --perf-log Enhancements

The current run had:

```text
perf.log = 0 bytes
```

because perf output appears to be emitted only at the end. This is insufficient for diagnosing long-running runs.

`--perf-log` should support both:

```text
1. final timing summary
2. streaming progress events during execution
```

### 12.1 Emit perf log incrementally

When `--perf-log` is enabled, write lines to stderr or the configured perf log file during execution.

Required behavior:

```text
- flush after each perf line or every small batch of perf lines
- include elapsed time since process start
- include current phase
- include row counters and SQLite sizes at chunk boundaries
```

Example:

```text
[perf] t=12.381s phase=refs.open elapsed=0.014s
[perf] t=12.429s phase=refs.load_contract elapsed=0.048s tables=9 truncated_sites=0
[perf] t=31.882s phase=scan.progress documents=100000 objects=432901 arrays=1022 scalars=981223 bytes_read=268435456
[perf] t=35.104s phase=flush.chunk index=12 shapes=41 fields=320 object_samples=200 value_samples=10000 elapsed=0.812s
[perf] t=35.891s phase=sqlite.prune_samples scope=value_priority touched_keys=831 rows_before=48210 rows_after=32000 elapsed=0.402s
```

### 12.2 Add scan progress counters

At periodic intervals:

```text
documents
objects
arrays
scalars
source bytes read
current document_index
current line number for JSONL
```

Suggested interval:

```text
every 100,000 JSONL documents
or every 256MB read
or every 10 seconds
```

Do not emit too frequently.

### 12.3 Add sample counters

At chunk flush:

```text
object_sample_pending_rows
value_sample_pending_rows
first_seen_count
first_non_empty_count
priority_sample_count
heavy_hitter_context_count
touched_object_sample_keys
touched_value_sample_fields
```

Example:

```text
[perf] t=144.203s phase=sample.chunk object_rows=812 value_rows=10000 heavy_hitter_context_rows=0 touched_value_fields=340
```

### 12.4 Add SQLite table row counters

At chunk flush or every N chunks:

```text
prof_shape rows
prof_shape_field rows
prof_object_sample rows
prof_field_value_sample rows
prof_field_summary rows
prof_field_value rows
```

Example:

```text
[perf] t=301.778s phase=sqlite.rows prof_shape=1458 prof_shape_field=8746 prof_object_sample=16819 prof_field_value_sample=70542
```

### 12.5 Add SQLite file size counters

At chunk flush or every N chunks:

```text
profile.sqlite bytes
profile.sqlite-wal bytes
profile.sqlite-shm bytes
```

Example:

```text
[perf] t=301.782s phase=sqlite.size db=214958080 wal=12582912 shm=163840
```

### 12.6 Add prune diagnostics

For each prune call:

```text
prune kind
scope
touched keys
rows before
rows after
deleted rows
elapsed
```

Example:

```text
[perf] t=390.120s phase=sqlite.prune kind=value_priority touched_fields=128 rows_before=24000 rows_after=1024 deleted=22976 elapsed=0.231s
```

This would have exposed the current problem early.

### 12.7 Add top table size diagnostic at optional low frequency

Every N chunks, optionally query `dbstat` when available.

This should be optional because `dbstat` can be expensive.

Example:

```text
[perf] t=600.000s phase=sqlite.dbstat top_table=prof_field_value_sample mb=1067.5
```

Recommended flag:

```text
--perf-log-dbstat
```

Keep separate from default `--perf-log`.

### 12.8 Emit final perf summary even on error

If the process exits due to an error, emit accumulated perf data before returning.

Example:

```text
[perf] final status=error elapsed=...
```

### 12.9 Suggested perf buckets

Minimum final buckets:

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
flush.shapes
flush.fields
flush.object_samples
flush.value_samples
sqlite.prune_object_samples
sqlite.prune_value_samples
sqlite.write_field_summaries
sqlite.write_field_values
sqlite.write_source_summary
sqlite.indexes
stdout.summary
```

Additional counters:

```text
documents_total
objects_total
arrays_total
scalars_total
shape_rows_total
shape_field_rows_total
object_sample_rows_total
value_sample_rows_total
heavy_hitter_context_rows_total
field_value_rows_total
exact_disabled_count
hll_field_count
space_saving_field_count
```

### 12.10 Perf log output destination

Current behavior writes `--perf-log` to stderr. That is acceptable.

For long runs, add optional:

```text
--perf-log-file <FILE>
```

If not implemented, shell users can redirect stderr:

```bash
profile-json-refs claude_merged.jsonl --perf-log 2> perf.log
```

But the tool should flush periodically so `perf.log` is useful during execution.

---

## 13. Proposed Safer Large-Input Defaults

Recommended default changes for v0.1.0:

```yaml
sampling:
  object:
    sample_json_limit_bytes: 16384
    canonical_path:
      priority_sample_limit: 1
    site_path:
      priority_sample_limit: 1
    field_set:
      priority_sample_limit: 2
    type_set:
      priority_sample_limit: 4

  value:
    value_json_limit_bytes: 1024
    parent_object_json_limit_bytes: 1024
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

For emergency large-run smoke:

```yaml
sampling:
  object:
    sample_json_limit_bytes: 2048
    canonical_path:
      priority_sample_limit: 1
    site_path:
      priority_sample_limit: 1
    field_set:
      priority_sample_limit: 1
    type_set:
      priority_sample_limit: 1

  value:
    value_json_limit_bytes: 256
    parent_object_json_limit_bytes: 512
    priority_sample_limit_per_field_profile: 1
    heavy_hitter_context_sample_limit: 0

value_profile:
  value_text_limit_bytes: 128
  exact_distinct_threshold: 32
  exact_value_bytes_per_field_profile: 32768
  global_exact_value_bytes_budget: 33554432
  hll_precision: 12
  heavy_hitter_limit: 16
```

---

## 14. Acceptance Criteria for the Fix

A patched run against the same input should satisfy:

```text
- heavy_hitter_context rows = 0 by default
- prof_field_value_sample stays bounded relative to field_profile_count and sample limits
- prof_field_value_sample does not dominate profile.sqlite by >80%
- prof_field_summary is populated
- prof_field_value is populated
- profile.sqlite growth is explainable by approved fact tables
- --perf-log emits useful progress before process completion
- SQLite prune time does not grow superlinearly with chunk count
```

Suggested SQL checks:

```sql
SELECT sample_kind, COUNT(*)
FROM prof_field_value_sample
GROUP BY sample_kind;

SELECT name, ROUND(SUM(pgsize) / 1024.0 / 1024.0, 2) AS mb
FROM dbstat
GROUP BY name
ORDER BY SUM(pgsize) DESC
LIMIT 20;

SELECT COUNT(*) FROM prof_field_summary;
SELECT COUNT(*) FROM prof_field_value;

SELECT COUNT(*)
FROM prof_field_value_sample
WHERE sample_kind = 'heavy_hitter_context';
```

Expected default:

```text
heavy_hitter_context = 0
```

### 14.1 Script-backed regression checks

Add a red regression guard before implementation:

```bash
cargo test rc2_diagnose_script_enforces_performance_safe_sample_contract -- --nocapture
```

Before the rc.2 code changes, this test should fail because the current implementation still emits `heavy_hitter_context` rows by default and can retain value priority samples above the rc.2 default limit.

The test uses:

```bash
scripts/diagnose_profile_sqlite.sh \
  --fail-on-risk \
  --hh-context-limit 0 \
  --value-sample-limit 4 \
  <profile.sqlite>
```

After the fix, this test must pass as part of `cargo test`.

For the full external regression, run:

```bash
PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
DUMP_JSON_REFS_BIN=dump-json-refs \
scripts/regression_profile_json_refs_v0_1_rc2_patch.sh
```

This is the rc.2 performance-safe regression harness. It validates the public v0.1 SQLite/CLI contract while checking that the performance fix prevents high-cardinality `heavy_hitter_context` growth and finalizes `prof_field_summary` / `prof_field_value`.

---

## 15. Final Decision

The current issue should be treated as a confirmed performance/design bug in the value sample subsystem.

Decision:

```text
- Stop the current run.
- Patch heavy_hitter_context policy first.
- Reduce parent_object_json defaults.
- Make sample materialization lazy.
- Restrict pruning to touched keys.
- Strengthen --perf-log to emit progress and counters during long runs.
```

Non-decision:

```text
Do not interpret this primarily as heterogeneous object array failure.
prof_shape and prof_shape_field counts are small in the observed DB.
```

v0.1.0 policy update:

```text
heavy_hitter_context is optional and disabled by default.
When enabled, it is finalization-only and limited to final surviving heavy hitter values.
```
