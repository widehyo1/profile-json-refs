# Profile JSON Refs Field/Value Hot Path Performance Plan

Recommended path:

```text
docs/plans/PROFILE_JSON_REFS_FIELD_VALUE_HOT_PATH_PLAN.md
```

## Status

This plan is for an immediate practical performance patch. It intentionally avoids broad architecture changes and external library adoption.

The current measurement shows that the dominant runtime cost is not JSON parsing or SQLite writing. The dominant cost is inside the scan/walk phase, specifically the field/value accumulator update path.

Measured symptoms from the 125k-line diagnostic run:

```text
documents:       125000
objects:          666088
arrays:           140142
scalars:         3377270
field_profiles:     8746
stored_values:    169054
elapsed:         ~210s
```

Approximate phase distribution:

```text
scan.chunk walk_ms:        dominant, ~89% of total runtime
scan.chunk parse_ms:       negligible, <1%
sqlite flush/prune:        secondary, ~10%
```

Sampled walk diagnostics narrowed the primary cost to the field update bucket:

```text
field/value accumulator update >> value hash / path / canonicalization / parse
```

The first patch should therefore focus on removing obvious hot-path work inside field/value observation.

## Scope

### In scope

- Reduce per-field-observation CPU cost.
- Preserve output semantics.
- Preserve SQLite schema.
- Preserve CLI behavior.
- Preserve deterministic IDs and hashes.
- Keep current approximate/exact profile behavior unless explicitly noted.
- Keep existing perf logging useful enough to compare before/after.

### Out of scope

- No external string interner library in this patch.
- No large identity interner rewrite.
- No parallel scan.
- No async SQLite writer.
- No SQLite schema migration.
- No change to public CLI options.
- No change to output row meaning.
- No approximate replacement of exact counters.
- No count-min sketch replacement.
- No HLL implementation replacement.

## Primary hypothesis

The main source-level bottleneck is in `FieldValueAccumulator::observe_inner()`.

Current simplified flow:

```text
field value observation
  -> update summary
  -> build ValueKey
  -> compute stable hash from formatted key
  -> HLL insert
  -> SpaceSaving heavy hitter observe
  -> ExactCounter observe
  -> store representative heavy hitter value
  -> prune heavy_hitter_values against active heavy hitter keys
  -> value sample observe
```

The strongest suspected costs are:

```text
1. heavy_hitter_values cleanup on every observation
2. duplicated ValueKey construction
3. SpaceSaving replacement path doing O(limit) victim scan
4. parent object clone for value sampling
5. repeated stable hash / value hash / sample hash work
```

This plan applies low-risk changes in that order.

## Required invariants

The patch must preserve these invariants:

```text
1. Same input produces the same profile row counts.
2. Stored values remain semantically equivalent.
3. Heavy hitter rows remain valid representatives of active heavy hitter keys.
4. Exact counting behavior remains unchanged where exact counting is enabled.
5. Value samples remain deterministic for the same input and configuration.
6. No hash-only identity may merge distinct logical values.
7. Perf diagnostics must not add map-wide scans to hot paths.
```

Before/after summary should match for the same fixture:

```text
documents
objects
arrays
scalars
canonical_paths
site_paths
shapes
field_profiles
stored_values
```

For large performance fixtures, exact byte-for-byte SQLite equality is not required unless existing tests already require it. Row counts and semantic output compatibility are required.

## Patch P0.1: Remove per-observation heavy_hitter_values retain

### Problem

Current code performs heavy hitter value cleanup on every field value observation.

Simplified current pattern:

```rust
self.heavy_hitters.observe(key.clone());

if self.heavy_hitters.contains_key(&key) {
    self.heavy_hitter_values.insert(key.clone(), value.clone());
}

let active_keys = self.heavy_hitters.keys();
self.heavy_hitter_values
    .retain(|key, _| active_keys.contains(key));
```

The cleanup is expensive because `heavy_hitters.keys()` materializes active keys, and the implementation may clone and sort keys. Performing this for every field value observation is too expensive.

### Goal

Only remove obsolete `heavy_hitter_values` entries when an eviction/replacement actually occurs, or defer cleanup to a bounded cleanup point.

### Preferred implementation

Change the SpaceSaving update API so it can report whether a key was evicted.

Introduce:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceSavingUpdate<K> {
    Existing,
    Inserted,
    Replaced { evicted: K },
}
```

Add a new method without removing the old one immediately:

```rust
impl<K> SpaceSaving<K>
where
    K: Clone + Eq + Ord + Hash,
{
    pub fn observe_update(&mut self, key: K) -> SpaceSavingUpdate<K> {
        // Preserve the existing SpaceSaving semantics.
        // Return Existing when the key was already present.
        // Return Inserted when the key was inserted without eviction.
        // Return Replaced { evicted } when a victim key was removed.
    }
}
```

Then update `FieldValueAccumulator::observe_inner()`:

```rust
match self.heavy_hitters.observe_update(key.clone()) {
    SpaceSavingUpdate::Existing | SpaceSavingUpdate::Inserted => {}
    SpaceSavingUpdate::Replaced { evicted } => {
        self.heavy_hitter_values.remove(&evicted);
    }
}

if self.heavy_hitters.contains_key(&key) {
    self.heavy_hitter_values.insert(key.clone(), value.clone());
}
```

Remove this hot-path cleanup:

```rust
let active_keys = self.heavy_hitters.keys();
self.heavy_hitter_values
    .retain(|key, _| active_keys.contains(key));
```

### Alternative implementation

If changing `SpaceSaving` is too invasive for the first patch, defer cleanup to finalization or flush preparation:

```rust
fn prune_heavy_hitter_values_to_active_keys(&mut self) {
    let active_keys = self.heavy_hitters.key_set_for_cleanup();
    self.heavy_hitter_values.retain(|key, _| active_keys.contains(key));
}
```

This is lower risk than per-observation cleanup, but the preferred implementation is eviction-driven removal because it keeps memory bounded more precisely.

### Tests

Add unit tests for `SpaceSaving::observe_update()`:

```text
- existing key returns Existing
- insert while below limit returns Inserted
- insert while full returns Replaced { evicted }
- after replacement, evicted key is not present
- counts preserve existing SpaceSaving semantics
```

Add unit/integration test for `FieldValueAccumulator`:

```text
- heavy_hitter_values does not retain values for evicted keys
- heavy_hitter_values contains representative values for active keys
- output heavy hitter rows remain stable for a deterministic fixture
```

### Validation metric

In perf logs after this patch, expect:

```text
scan.chunk walk_ms decreases
sampled_walk field_update_ms decreases
stored_values remains unchanged
SQLite row counts remain compatible
```

## Patch P0.2: Reuse ValueKey in ExactCounter

### Problem

`FieldValueAccumulator::observe_inner()` computes a `ValueKey`, then `ExactCounter::observe(value)` computes a second `ValueKey` internally.

Simplified current pattern:

```rust
let key = value_key(value);
...
self.exact.observe(value);
```

Inside `ExactCounter::observe()`:

```rust
let key = value_key(value);
```

For string-heavy data, this means repeated string cloning and key construction.

### Goal

Compute `ValueKey` once per field value observation and share it across HLL, heavy hitter, exact counter, and sampling where practical.

### Implementation

Add a keyed exact-counter API:

```rust
impl ExactCounter {
    pub fn observe_keyed(&mut self, key: &ValueKey, value: &Value) {
        if !self.enabled {
            return;
        }

        // Preserve existing cap / disabling behavior.
        // Use key.clone() only when the key must be inserted or updated.
    }
}
```

Then change the field accumulator:

```rust
let key = value_key(value);
...
self.exact.observe_keyed(&key, value);
```

Keep the old `observe(value)` method if other call sites use it. It may delegate to the keyed version:

```rust
pub fn observe(&mut self, value: &Value) {
    if !self.enabled {
        return;
    }
    let key = value_key(value);
    self.observe_keyed(&key, value);
}
```

### Tests

Existing exact-counter tests should continue to pass.

Add a test that compares old and keyed behavior:

```text
- observe(value) and observe_keyed(value_key(value), value) produce the same counts
- exact cap/disabling behavior is unchanged
```

### Validation metric

Expect lower sampled `field_update_ms`. The effect should be larger on string-heavy inputs.

## Patch P0.3: Replace debug-format stable hash input

### Problem

The current HLL hash path may derive stable hash bytes using debug formatting of `ValueKey`:

```rust
let hash64 = stable_u64(format!("{key:?}").as_bytes());
```

This allocates a temporary `String` and formats the key for every observation.

### Goal

Compute a stable hash for `ValueKey` without debug formatting and without temporary string allocation.

### Implementation

Add a stable hashing method on `ValueKey`:

```rust
impl ValueKey {
    pub fn stable_hash64(&self) -> u64 {
        // Must preserve type separation.
        // For example, string "1" and number 1 must not collide by construction.
        // Use explicit type tags and stable byte representation.
    }
}
```

Conceptual encoding:

```text
Null       -> tag b'n'
Bool false -> tag b'b' + b'0'
Bool true  -> tag b'b' + b'1'
Number     -> tag b'd' + canonical numeric representation
String     -> tag b's' + UTF-8 bytes
Array/Object fallback, if represented -> explicit tag + canonical representation
```

Then change:

```rust
let hash64 = key.stable_hash64();
self.hll.insert_hash(hash64);
```

Do not use Rust's default `Hash` output for persisted or cross-run semantics. Default hashers are not stable across runs and must not be used for persisted identity.

### Tests

Add tests for stable hash behavior:

```text
- same ValueKey produces same hash across calls
- different type tags do not intentionally share byte encoding
- string "1" and number 1 have different hash input semantics
- bool true, string "true", and number 1 are distinct
```

Do not assert specific hash numbers unless the project already treats stable hash output as part of the compatibility contract.

### Validation metric

Expect lower sampled `field_update_ms` and possibly lower `value_hash_ms` if separately measured.

## Patch P0.4: Defer parent object clone for value samples

### Problem

The current observation path may clone the parent object before knowing whether the sample row will be retained.

Simplified current pattern:

```rust
let parent_object = Value::Object(object.clone());
...
value_samples.observe(..., parent_object, ...);
```

This is expensive for object-heavy JSONL.

### Goal

Avoid cloning the parent object unless a sample row is actually materialized and retained by the sample accumulator.

### Implementation

Change value sample observation to borrow the parent object during the observation call:

```rust
pub struct ValueSampleObservation<'a> {
    pub document_index: u64,
    pub field_profile_id: &'a str,
    pub source_path: SourcePathRef<'a>,
    pub value: &'a Value,
    pub parent_object: &'a serde_json::Map<String, Value>,
}
```

The sample accumulator should create owned row data only if the observation is accepted into its bounded candidate set.

Current conceptual flow:

```text
observe field
  -> clone parent object
  -> build sample row
  -> maybe keep row
```

Target conceptual flow:

```text
observe field
  -> compute deterministic sample priority
  -> check whether candidate can enter top-k
  -> only if accepted, materialize sample row
       -> serialize parent_object
       -> clone/store value if needed
       -> store owned source_path string
```

This may require splitting `observe()` into two steps:

```rust
fn should_accept_candidate(&self, priority: SamplePriority) -> bool;
fn materialize_row(observation: ValueSampleObservation<'_>) -> Result<ValueSampleRow>;
```

Keep deterministic sampling behavior unchanged.

### Tests

Add tests for sample determinism:

```text
- same fixture produces same accepted sample source paths
- same fixture produces same sample count
- rejected candidates do not require parent object materialization
```

If direct clone-count testing is difficult, rely on semantic tests plus perf validation.

### Validation metric

Expect lower walk time on object-heavy inputs. The effect depends on how often current code clones parent objects for rejected sample candidates.

## Patch P0.5: Avoid repeated value hash / sample hash work

### Problem

The field observation path can compute related hashes more than once:

```text
ValueKey hash for HLL
value_hash for stored value/sample identity
sample priority hash
```

Some of these are logically different and must remain distinct, but the input preparation should not be duplicated unnecessarily.

### Goal

Introduce a lightweight per-observation context that computes reusable derived value information once.

### Implementation

Introduce an internal struct:

```rust
pub struct ObservedValue {
    pub key: ValueKey,
    pub stable_hash64: u64,
    pub value_type: JsonType,
}
```

Create it once:

```rust
let observed = ObservedValue::from_value(value);
```

Use it across subsystems:

```rust
self.hll.insert_hash(observed.stable_hash64);
self.heavy_hitters.observe_update(observed.key.clone());
self.exact.observe_observed(&observed, value);
self.value_samples.observe_observed(&observed, ...);
```

Do not over-expand this patch. The initial goal is to avoid repeated key/hash construction, not to redesign all value handling.

### Tests

Add equivalence tests:

```text
- ObservedValue::from_value(value).key == value_key(value)
- observed stable hash is consistent with ValueKey::stable_hash64()
- accumulator output is unchanged for a deterministic fixture
```

## Patch P0.6: Keep scan diagnostics low-overhead

### Problem

Previous diagnostic changes caused major overhead by adding aggregate map scans to object hot paths.

### Rule

Perf diagnostics must not call aggregate counting helpers in hot paths.

Forbidden in per-object/per-field/per-scalar paths:

```text
pending_value_sample_count()
pending_object_sample_count()
heavy_hitters.keys()
any helper that scans a HashMap/BTreeMap/Vec of accumulated state
any helper that sorts accumulated keys
```

Allowed:

```text
increment counters from values already available at the current visit point
sampled timing for one document every N documents
phase-level timing around existing function boundaries
```

### Required perf fields after patch

Keep these fields available:

```text
phase=scan.chunk parse_ms=... walk_ms=...
phase=scan.hot_counters field_updates=... value_observations=... value_hashes=...
phase=scan.sampled_walk field_update_ms=...
phase=flush.trigger reason=...
phase=sqlite.prune.* rows_deleted=...
```

## Recommended implementation order

Apply in this exact order, measuring after each step if possible:

```text
1. SpaceSaving observe_update + remove per-observation heavy_hitter_values retain
2. ExactCounter observe_keyed
3. ValueKey stable_hash64 without debug formatting
4. ObservedValue minimal context
5. Parent object clone deferral for value samples
6. Keep/adjust diagnostics without adding hot-path aggregate scans
```

Rationale:

```text
Step 1 removes the strongest source-level hot-path anti-pattern.
Step 2 removes duplicated key construction with low semantic risk.
Step 3 removes repeated allocation/formatting in hash input.
Step 4 consolidates the improvements without a broad rewrite.
Step 5 is valuable but may touch sample accumulator lifetimes and should come after lower-risk patches.
```

## Benchmark protocol

Use the 125k-line diagnostic fixture first.

Example:

```bash
/usr/bin/time -v rprofile-json-refs claude_merged_measure.jsonl --perf-log --perf-log-dbstat 2>perf_after.time.log
```

Capture:

```text
stdout summary
/usr/bin/time -v fields
perf log
profile.sqlite dbstat
```

Compare against the current baseline:

```text
elapsed: ~209.810s on 125k fixture
```

Required comparison fields:

```text
elapsed
User time
System time
Percent of CPU
Maximum resident set size
scan.read_parse_walk
sum(scan.chunk parse_ms)
sum(scan.chunk walk_ms)
sum(scan.sampled_walk field_update_ms)
sum(sqlite.flush.*)
sum(sqlite.prune.*)
rows_deleted totals
```

Then run the 500k fixture after the 125k result is promising.

## Success criteria

Minimum acceptable success:

```text
- No output summary regression.
- No schema regression.
- No stored_values regression.
- 125k elapsed improves by at least 15%.
- sampled field_update_ms decreases materially.
```

Good success:

```text
- 125k elapsed improves by 25% or more.
- 500k elapsed scales better than before.
- walk_ms per scalar decreases.
- field_update_ms remains the largest sampled bucket but is significantly lower.
```

Failure indicators:

```text
- stored_values changes unexpectedly.
- heavy hitter rows lose representative values.
- exact counter behavior changes.
- perf-log build becomes slower due to new diagnostics.
- memory grows substantially because obsolete heavy_hitter_values are no longer cleaned up.
```

## Rollback plan

Each patch should be committed separately.

Suggested commits:

```text
perf(field): report field update diagnostics without aggregate scans
perf(value): remove per-observation heavy hitter cleanup
perf(value): reuse value keys in exact counter
perf(value): avoid debug formatting for stable value hash
perf(sample): defer parent object materialization
```

If a semantic regression appears, revert the latest patch only.

If memory grows after removing per-observation cleanup, keep the SpaceSaving eviction-driven cleanup. If eviction-driven cleanup is not ready, add bounded cleanup at flush/finalize as a temporary fallback.

## Codex implementation instruction

Use the following implementation constraints:

```text
Implement the immediate field/value hot path performance patch.

Do not add external dependencies.
Do not change CLI options.
Do not change SQLite schema.
Do not change output semantics.
Do not introduce broad interner architecture in this patch.

Primary tasks:
1. Add SpaceSaving::observe_update() returning Existing / Inserted / Replaced { evicted }.
2. Remove per-observation heavy_hitter_values cleanup based on heavy_hitters.keys().
3. Remove obsolete heavy_hitter_values entries only when SpaceSaving reports an evicted key, or at bounded cleanup points if necessary.
4. Add ExactCounter::observe_keyed() and use the already computed ValueKey from FieldValueAccumulator::observe_inner().
5. Replace stable_u64(format!("{key:?}").as_bytes()) with a stable non-allocating ValueKey hash method.
6. Introduce a minimal ObservedValue only if it reduces duplicated key/hash construction without broad rewrite.
7. Defer parent object cloning in value sampling if it can be done without changing deterministic sampling behavior.
8. Keep perf diagnostics low-overhead. Do not call aggregate count helpers or key materialization helpers from per-object/per-field/per-scalar hot paths.

Add or update tests for:
- SpaceSaving update result semantics.
- ExactCounter keyed observation equivalence.
- ValueKey stable hash type separation.
- FieldValueAccumulator semantic equivalence on deterministic fixtures.
- Perf log still includes scan.chunk, scan.sampled_walk, flush.trigger, and prune rows_deleted fields.

After implementation, run:
- cargo test
- the 125k perf fixture with --perf-log --perf-log-dbstat

Report before/after:
- elapsed
- walk_ms total
- sampled field_update_ms total
- sqlite flush/prune totals
- stored_values
- field_profiles
```

## Notes for future work

This patch deliberately stops before a full internal ID/interner rewrite. If this patch confirms that field/value updates remain the dominant cost, the next performance plan should introduce internal IDs for repeated strings, paths, shapes, and field profiles.

Do not include that larger rewrite in this patch.
