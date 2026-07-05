# profile-json-refs v0.1.2 Plan: O(1) Pending Value Sample Counter

Recommended repository path:

```text
docs/plans/PROFILE_JSON_REFS_V0_1_2_PENDING_SAMPLE_COUNTER_PLAN.md
```

## 1. Context

`v0.1.1` has been tagged after the field/value hot-path patch:

```bash
git tag -a v0.1.1 -m "v0.1.1"
```

The `v0.1.1` patch removed the dominant `FieldValueAccumulator` bottleneck. On the 125k-line measurement input, the important observed change was:

```text
walk_ms:              ~187s -> ~40s
sampled field_update: ~129ms -> ~7ms
SQLite flush/prune:   roughly flat
```

After this improvement, the next suspected walk-side cost is the global pending sample count used for flush decisions.

Current suspected pattern:

```text
flush decision
  -> pending_value_sample_count()
  -> iterate all field value accumulators
  -> sum pending value sample rows
```

With `field_value_accumulators ~= 8746`, repeatedly scanning all accumulators is now a likely remaining walk-side cost. The goal for `v0.1.2` is to replace global recounting with an O(1) maintained counter.

## 2. Goal

Introduce an O(1) pending value sample row counter for flush decisions.

Target behavior:

```text
field value sample observation
  -> update exactly one field accumulator
  -> compute local pending sample row delta
  -> update run-level pending_value_sample_rows counter

flush decision
  -> compare run-level pending_value_sample_rows against configured limit
  -> no global scan over all field value accumulators
```

## 3. Non-goals

Do not include these in `v0.1.2`:

```text
- external library adoption
- string interner adoption
- schema / shape interner adoption
- SQLite schema changes
- output format changes
- sample policy changes
- prune hysteresis
- async writer
- parallel scan
- field_profile_id or source_path materialization rewrite
```

This patch should be a narrow performance patch over the `v0.1.1` baseline.

## 4. Required invariants

The patch must preserve all profiling semantics.

For the same input and options, the following must remain unchanged:

```text
- stdout summary counts
- SQLite schema
- SQLite logical rows
- stored_values count
- field_profiles count
- shape count
- sample limits
- flush result semantics
- finalization behavior
- cargo test behavior
```

The counter is an implementation detail only.

## 5. Current problem

A likely current implementation shape is:

```rust
fn pending_value_sample_count(&self) -> usize {
    self.field_values
        .values()
        .map(FieldValueAccumulator::pending_value_sample_count)
        .sum()
}
```

This is O(number of field value accumulators). It is acceptable for diagnostics or final assertions, but not for a hot-path flush decision.

The problematic usage is any flush trigger check like:

```rust
if self.pending_value_sample_count() >= self.config.flush.chunk_value_sample_rows {
    self.flush_chunk(FlushReason::ValueSampleLimit)?;
}
```

This turns a simple threshold check into a repeated scan over all field value accumulators.

## 6. Target design

Add a run-level counter owned by the scan/profile visitor.

Example:

```rust
struct ProfileRunVisitor {
    // existing fields...
    pending_value_sample_rows: usize,
}
```

Flush checks become O(1):

```rust
if self.pending_value_sample_rows >= self.config.flush.chunk_value_sample_rows {
    self.flush_chunk(FlushReason::ValueSampleLimit)?;
}
```

The counter must be updated only when pending value sample rows are added, removed, drained, or replaced.

## 7. Preferred implementation strategy

### 7.1 Return local delta from value sample observation

The most robust design is to make the local update return a delta.

Recommended type:

```rust
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct PendingRowDelta {
    added: usize,
    removed: usize,
}

impl PendingRowDelta {
    fn net(self) -> isize {
        self.added as isize - self.removed as isize
    }
}
```

Or, if pending sample count can only increase before a drain:

```rust
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct FieldValueObserveStats {
    pending_value_sample_rows_added: usize,
}
```

Use the more general `added/removed` form if the sample accumulator can replace or evict pending candidates.

### 7.2 Add observe stats at the right layer

Recommended flow:

```text
ProfileRunVisitor::observe_object_fields
  -> FieldValueAccumulator::observe_observed(...)
    -> ValueSampleAccumulator::observe_observed(...)
       -> returns PendingRowDelta or rows_added
  -> visitor updates pending_value_sample_rows
```

Example:

```rust
let stats = accumulator.observe_observed(...)?;
self.apply_pending_value_sample_delta(stats.pending_value_sample_delta);
```

Counter update helper:

```rust
fn apply_pending_value_sample_delta(&mut self, delta: PendingRowDelta) {
    self.pending_value_sample_rows += delta.added;
    self.pending_value_sample_rows = self
        .pending_value_sample_rows
        .checked_sub(delta.removed)
        .expect("pending value sample counter underflow");
}
```

If only `rows_added` is needed:

```rust
self.pending_value_sample_rows += stats.pending_value_sample_rows_added;
```

### 7.3 Avoid global recounting after each observation

Do not implement this pattern:

```rust
let before = self.pending_value_sample_count();
accumulator.observe(...)?;
let after = self.pending_value_sample_count();
self.pending_value_sample_rows += after - before;
```

That preserves the O(N) cost and defeats the purpose.

A temporary per-accumulator before/after is acceptable only if both calls are O(1) on that single accumulator:

```rust
let before = accumulator.pending_value_sample_count(); // must be O(1)
accumulator.observe(...)?;
let after = accumulator.pending_value_sample_count();  // must be O(1)
self.pending_value_sample_rows += after.saturating_sub(before);
```

However, the preferred implementation is still to return an explicit local delta from the sample observation path.

## 8. Drain / flush behavior

The counter must be reset or decremented when pending value sample rows are drained into a flush chunk.

Preferred approach:

```rust
let value_samples = self.drain_pending_value_samples();
let drained = value_samples.len();
self.pending_value_sample_rows = self
    .pending_value_sample_rows
    .checked_sub(drained)
    .expect("pending value sample counter underflow during drain");
```

If the current flush code drains all pending value samples at once, a simpler reset is acceptable:

```rust
let value_samples = self.drain_pending_value_samples();
debug_assert_eq!(value_samples.len(), self.pending_value_sample_rows);
self.pending_value_sample_rows = 0;
```

The subtract form is more robust if future selective flushing is added.

## 9. Debug-only consistency check

Keep the existing global count function, but remove it from hot-path flush decisions.

Recommended use:

```rust
#[cfg(debug_assertions)]
fn debug_assert_pending_value_sample_counter(&self) {
    let actual = self.pending_value_sample_count_slow();
    debug_assert_eq!(
        self.pending_value_sample_rows,
        actual,
        "pending value sample counter drifted"
    );
}
```

Name the slow function explicitly:

```rust
fn pending_value_sample_count_slow(&self) -> usize {
    self.field_values
        .values()
        .map(FieldValueAccumulator::pending_value_sample_count)
        .sum()
}
```

Do not call this function in release hot paths.

## 10. Perf-log updates

`flush.trigger` should report the O(1) counter value:

```text
phase=flush.trigger ... pending_value_samples=<self.pending_value_sample_rows>
```

`scan.accumulators` should also use the O(1) counter where it currently reports pending value samples.

Optional debug-only diagnostic field:

```text
pending_value_samples_slow=<slow_count>
```

Do not add the slow field in normal `--perf-log` release behavior unless explicitly compiled for diagnostic builds. It would reintroduce the cost.

## 11. Tests

### 11.1 Unit tests

Add focused tests around the counter behavior if the relevant types are accessible.

Cases:

```text
- initial pending_value_sample_rows is 0
- one accepted value sample increments the counter
- non-accepted sample does not increment the counter
- sample replacement does not incorrectly grow the counter
- drain subtracts or resets the counter
- counter never underflows
```

### 11.2 Integration tests

Use a small fixture that triggers value sample flush.

Assertions:

```text
- command exits successfully
- stdout summary remains stable
- perf log contains phase=flush.trigger
- perf log contains reason=value_sample_limit when threshold is reached
- perf log reports pending_value_samples=
- generated profile.sqlite has the same logical counts as before
```

Avoid exact elapsed-time assertions.

### 11.3 Regression comparison

Against `v0.1.1`, verify:

```bash
# v0.1.1 binary
profile-json-refs input.jsonl --perf-log --perf-log-dbstat 2>before.log
sqlite3 profile.sqlite '.dump' > before.sql

# v0.1.2 candidate binary
profile-json-refs input.jsonl --perf-log --perf-log-dbstat 2>after.log
sqlite3 profile.sqlite '.dump' > after.sql

diff -u before.sql after.sql
```

If volatile metadata exists in the dump, compare table counts and stable ordered rows instead.

## 12. Manual benchmark protocol

Use the existing 125k measurement fixture first.

```bash
/usr/bin/time -v profile-json-refs claude_merged_measure.jsonl --perf-log --perf-log-dbstat 2>perf_v0_1_2.log
```

Capture:

```text
- stdout elapsed
- /usr/bin/time wall time
- User time
- System time
- Percent of CPU
- Maximum resident set size
- scan.chunk walk_ms sum
- scan.chunk parse_ms sum
- sqlite.flush.* sum
- sqlite.prune.* sum
- flush.trigger reason distribution
```

Expected outcome:

```text
- stdout summary counts unchanged
- SQLite logical output unchanged
- walk_ms decreases or remains flat
- SQLite flush/prune remains roughly flat
- no increase in RSS large enough to matter
```

If `walk_ms` does not improve, still keep the patch if it removes a proven O(N) hot-path check and all invariants pass. It is a structural improvement and prevents future degradation as `field_value_accumulators` grows.

## 13. Implementation checklist

```text
[ ] Add run-level pending_value_sample_rows counter
[ ] Rename global count helper to pending_value_sample_count_slow or equivalent
[ ] Remove slow global count from flush trigger checks
[ ] Update FieldValueAccumulator / ValueSampleAccumulator observe path to return local pending row delta
[ ] Update run-level counter from local delta
[ ] Decrement or reset counter during drain/flush
[ ] Add debug_assert consistency check against slow count
[ ] Update perf-log fields to report O(1) counter
[ ] Add unit tests for counter drift and drain behavior
[ ] Add integration test for flush.trigger pending_value_samples
[ ] Run cargo fmt
[ ] Run cargo clippy if project already uses it
[ ] Run cargo test
[ ] Run 125k benchmark
[ ] Compare profile.sqlite logical output with v0.1.1
```

## 14. Suggested commit structure

```text
perf(profile): maintain pending value sample count in O(1)
test(profile): cover pending value sample counter drift
docs(perf): add v0.1.2 pending sample counter plan
```

If the implementation is small, a single commit is acceptable:

```text
perf(profile): avoid global pending sample recounts
```

## 15. v0.1.2 release checklist

After implementation and benchmark verification:

```bash
cargo fmt
cargo test
cargo package
```

Review package contents:

```bash
cargo package --list
```

Update version metadata to `0.1.2`:

```text
Cargo.toml
Cargo.lock if applicable
README or changelog if the repository keeps release notes
```

Create the annotated tag:

```bash
git tag -a v0.1.2 -m "v0.1.2"
```

Push branch and tag:

```bash
git push
git push origin v0.1.2
```

Publish:

```bash
cargo publish
```

If `cargo publish` fails because the package was already uploaded or metadata is stale, do not retag without deciding whether to:

```text
- delete and recreate the local tag before pushing, if not pushed yet
- create v0.1.3 instead, if v0.1.2 was already pushed/published incorrectly
```

## 16. Rollback criteria

Revert the patch if any of the following occurs:

```text
- profile.sqlite logical output changes
- stored_values changes
- field_profiles changes
- sample rows differ unexpectedly
- value_sample_limit flush behavior changes in a way not explained by equivalent chunking
- counter drift assertion fails
- benchmark regresses materially versus v0.1.1
```

Do not revert merely because SQLite flush/prune remains unchanged. This patch targets walk-side pending count overhead, not SQLite cost.

## 17. Next tasks after v0.1.2

Do not combine these with `v0.1.2`, but keep them as next candidates:

```text
1. SQLite prune hysteresis
2. O(1) pending object sample counter, if a similar global recount exists
3. field_profile_id / source_path materialization delay
4. shape facts interning or shape signature cache
```
