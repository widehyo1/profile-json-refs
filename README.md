# profile-json-refs

`profile-json-refs` is a value-level profiling tool for JSON and JSONL snapshots.

It runs downstream of [`dump-json-refs`](https://github.com/widehyo1/dump-json-refs). `dump-json-refs` produces structural refs in `refs/schemas.sqlite`; `profile-json-refs` consumes that refs database together with the original JSON/JSONL source file and writes `profile.sqlite`.

```text
JSON / JSONL source
        │
        ├── dump-json-refs ───────► refs/schemas.sqlite
        │
        └── profile-json-refs ────► profile.sqlite
                 ▲
                 └── consumes refs/schemas.sqlite
```

The goal is to produce best-effort value-level facts for human inspection and UX presentation.

---

## Why this exists

`dump-json-refs` answers structural questions:

```text
- Which canonical paths and site paths exist?
- Which schema paths describe each object location?
- Which fields are present or missing?
- Which field combinations appear?
- Which field names have multiple observed types?
```

`profile-json-refs` adds value-level facts:

```text
- What values appear under this field?
- Is this field mostly unique?
- Does this field look categorical?
- Which values are frequent enough to inspect?
- Which object samples represent each navigation level?
- Does the same field name behave differently across shapes or types?
```

These facts are useful when reverse-engineering a single JSON/JSONL snapshot into a relational or tabular representation.

---

## Scope

`profile-json-refs` profiles one input snapshot at a time.

It does not track lineage across snapshots. For a different source snapshot, run `dump-json-refs` and `profile-json-refs` again.

### Inputs

```text
required:
  - original JSON or JSONL source file
  - refs/schemas.sqlite produced by dump-json-refs
```

### Output

```text
profile.sqlite
```

The output database contains `prof_*` fact tables. It is a one-shot artifact, not a run history database.

---

## Non-goals

`profile-json-refs` does not own:

```text
- generating refs
- producing refs/schemas.sqlite
- rendering refs JSON files or site path symlinks
- accepting stdin in v0.1.0
- generating DBML, SQL DDL, or parquet
- deciding final table boundaries
- deciding primary keys or foreign keys
- tracking lineage across snapshots
```

It may expose facts that help inspect candidate keys, categorical values, sparse fields, shape variants, and value distributions. It does not make final materialization decisions.

---

## Shape-aware profiling

A value is profiled in structural context. The same field name may appear under different paths, schema paths, field combinations, or observed type contexts.

The expected inspection path is:

```text
canonical path
  -> site path
    -> field combination
      -> shape with type
        -> object samples
        -> field value profile
```

`profile-json-refs` preserves the keys needed for this path:

```text
canonical_path
site_path
schema_path
field_set_hash
type_set_hash
shape_id
field_profile_id
value_hash
```

---

## Sampling

Sampling has two responsibilities:

```text
1. guarantee that every materialized navigation key has at least one source-backed sample;
2. provide bounded representative samples without unbounded memory growth.
```

Object samples are stored in `prof_object_sample` for these grains:

```text
canonical_path
canonical_path + site_path
canonical_path + site_path + field_set_hash
canonical_path + site_path + field_set_hash + type_set_hash
```

Each materialized key gets a mandatory `first_seen` sample. The scanner also records a best-effort `first_non_empty` sample when a structurally non-empty candidate appears. Additional representative samples are selected with chunk-mergeable deterministic priority sampling.

This avoids the problem where the first sample is `{}` or `[]` while later rows contain meaningful structure.

---

## Value-level facts

Important field-level facts include:

```text
- profiled count
- null and non-null counts
- empty object / empty array counts
- approximate or exact distinct count
- stored value count
- frequent value candidates
- source-backed value samples
- value text truncation status
- value source and count method
```

`profile-json-refs` should prefer observable facts over opaque scores.

---

## Algorithms

`profile-json-refs` uses bounded algorithms to handle large finite JSON/JSONL inputs:

```text
HyperLogLog:
  approximate distinct count per field profile

Space-Saving:
  heavy hitter candidates for frequent values

Deterministic priority sampling:
  chunk-mergeable bounded object/value samples

Bounded exact counters:
  exact full distribution for small field profiles
```

Approximate and sampled facts are always labeled. A missing value row does not necessarily mean the value was never observed; it may only mean it was not retained by the bounded profile.

---

## Basic usage

```bash
dump-json-refs data.jsonl --jsonl --outdir refs
profile-json-refs data.jsonl --jsonl
```

Defaults:

```text
--refs refs/schemas.sqlite
--out  profile.sqlite
```

Default stdout prints only the output path, `prof_source_summary`-level counts, and elapsed time. Detailed inspection should use `profile.sqlite`.
