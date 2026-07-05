# profile-json-refs

`profile-json-refs` profiles value-level facts from JSON and JSONL snapshots.

It is designed to run after [`dump-json-refs`](https://github.com/widehyo1/dump-json-refs). `dump-json-refs` extracts structural reference facts into `refs/schemas.sqlite`; `profile-json-refs` reads the original source file together with that refs database and writes a value-profile database, `profile.sqlite`.

```text
JSON / JSONL source
        │
        ├── dump-json-refs ───────► refs/schemas.sqlite
        │
        └── profile-json-refs ────► profile.sqlite
                 ▲
                 └── reads refs/schemas.sqlite
```

The purpose is to make a finite JSON/JSONL snapshot easier to inspect, reverse-engineer, and present in higher-level UX layers. It records observable facts about values, field profiles, object samples, and shape-aware value distributions. It does not decide the final relational model.

## Why this exists

`dump-json-refs` answers structural questions:

```text
- Which canonical paths and site paths exist?
- Which schema paths describe each object location?
- Which fields are present or missing?
- Which field combinations appear?
- Which field names have multiple observed types?
```

`profile-json-refs` adds value-level questions:

```text
- What values appear under this field profile?
- Is this field mostly unique, mostly null, or mostly categorical?
- Which values are frequent enough to inspect?
- Which source-backed samples represent this object or field?
- Does the same field name behave differently across shapes, paths, or observed types?
```

These facts are useful when turning raw JSON/JSONL into candidate tables, columns, categories, validation rules, and human-readable inspection views.

## Scope

`profile-json-refs` profiles one source snapshot at a time.

Inputs:

```text
- a JSON or JSONL source file
- a refs SQLite database produced by dump-json-refs
```

Output:

```text
profile.sqlite
```

The output database contains `prof_*` fact tables. It is a materialized profile artifact for the current source snapshot, not a migration log or multi-run history database.

## Non-goals

`profile-json-refs` does not own:

```text
- generating structural refs
- rendering refs JSON files or site-path symlinks
- deciding final table boundaries
- deciding primary keys or foreign keys
- generating DBML, SQL DDL, parquet, or application code
- tracking lineage across multiple snapshots
- merging profiles from multiple runs
```

It provides facts that can support those later decisions.

## Shape-aware profiling

A value is profiled in structural context. The same field name may have different meaning, type behavior, nullability, or value distribution depending on where and how it appears.

The expected inspection path is:

```text
canonical path
  -> site path
    -> field combination
      -> shape with observed field types
        -> object samples
        -> field value profiles
```

The profile keeps stable keys needed for this navigation path:

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

## Value-level facts

The profile database is intended to expose observable, source-backed facts such as:

```text
- profiled count
- null and non-null counts
- empty object and empty array counts
- observed type distribution
- exact or approximate distinct count
- retained value count
- frequent value candidates
- representative value samples
- representative object samples
- value text truncation status
- source path for retained samples
- count method used for retained values
```

Approximate or sampled facts should be read as bounded profile facts, not as a complete dump of every observed value.

## Sampling and bounded profiling

Large JSON/JSONL inputs can contain many repeated objects, high-cardinality fields, and large scalar values. `profile-json-refs` uses bounded profiling techniques so that the output stays inspectable and finite.

The profiler may use:

```text
- approximate distinct counters for field-level cardinality
- bounded exact counters for small distributions
- heavy-hitter tracking for frequent value candidates
- deterministic priority sampling for bounded object and value samples
- size limits for stored JSON/text fragments
```

Sampling has two responsibilities:

```text
1. keep source-backed examples for materialized navigation keys;
2. avoid unbounded memory and output growth on large snapshots.
```

A missing retained value row does not prove that the value never appeared. It may only mean the value was not retained by the bounded profile policy.

## CLI contract

Basic usage:

```bash
dump-json-refs data.jsonl --jsonl --outdir refs
profile-json-refs data.jsonl --jsonl
```

Default paths:

```text
refs database: refs/schemas.sqlite
output:        profile.sqlite
```

Common options:

```bash
profile-json-refs data.jsonl --jsonl
profile-json-refs data.json  --refs refs/schemas.sqlite --out profile.sqlite
profile-json-refs data.jsonl --perf-log --perf-log-dbstat 2>perf.log
```

The command writes the profile database and prints a compact summary to stdout:

```text
profile-json-refs: wrote profile.sqlite

documents: ...
objects: ...
arrays: ...
scalars: ...
canonical_paths: ...
site_paths: ...
shapes: ...
field_profiles: ...
stored_values: ...
elapsed: ...s
```

Performance diagnostics are written to stderr when enabled. They are diagnostic output, not part of the stable data artifact.

## Output artifact

`profile.sqlite` is the primary artifact. Downstream tools should inspect the `prof_*` tables rather than scraping stdout.

The stdout summary is intended for quick command-line confirmation. The SQLite database is the contract for further inspection, UI layers, and later decision/modeling tools.
