# Data Model

This document defines the physical SQLite tables written to `profile.sqlite` in v0.1.0.

`profile.sqlite` is a one-shot artifact. It has no run table, no manifest table, no source id, no profile run id, no algorithm table, no warning table, and no views.

---

## Table Set

```text
prof_source_summary
prof_object_sample
prof_shape
prof_shape_field
prof_field_summary
prof_field_value
prof_field_value_sample
```

---

## prof_source_summary

Single summary row. No `id` column.

```sql
CREATE TABLE prof_source_summary (
    source_format TEXT NOT NULL CHECK (source_format IN ('json', 'jsonl', 'unknown')),
    total_document_count INTEGER CHECK (total_document_count >= 0),
    total_object_count INTEGER CHECK (total_object_count >= 0),
    total_array_count INTEGER CHECK (total_array_count >= 0),
    total_scalar_count INTEGER CHECK (total_scalar_count >= 0),
    total_canonical_path_count INTEGER CHECK (total_canonical_path_count >= 0),
    total_site_path_count INTEGER CHECK (total_site_path_count >= 0),
    total_shape_count INTEGER CHECK (total_shape_count >= 0),
    total_field_profile_count INTEGER CHECK (total_field_profile_count >= 0),
    total_stored_value_count INTEGER CHECK (total_stored_value_count >= 0)
);
```

---

## prof_object_sample

`prof_object_sample` stores source-backed object samples for navigation grains.

Sample grains:

```text
canonical_path:
  key = canonical_path

site_path:
  key = canonical_path + site_path

field_set:
  key = canonical_path + site_path + field_set_hash

type_set:
  key = canonical_path + site_path + field_set_hash + type_set_hash
```

```sql
CREATE TABLE prof_object_sample (
    object_sample_id TEXT PRIMARY KEY,

    sample_scope TEXT NOT NULL CHECK (
        sample_scope IN ('canonical_path', 'site_path', 'field_set', 'type_set')
    ),

    sample_key TEXT NOT NULL,

    canonical_path TEXT NOT NULL,
    site_path TEXT,
    schema_path TEXT,

    field_set_hash TEXT,
    type_set_hash TEXT,
    shape_id TEXT,

    sample_kind TEXT NOT NULL CHECK (
        sample_kind IN ('first_seen', 'first_non_empty', 'priority_sample')
    ),

    document_index INTEGER,
    source_path TEXT,

    sample_json TEXT NOT NULL,
    sample_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (sample_json_truncated IN (0, 1)),

    sample_is_empty_object INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_object IN (0, 1)),
    sample_is_empty_array INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_array IN (0, 1)),

    sample_priority INTEGER,
    sample_rank INTEGER
);

CREATE UNIQUE INDEX idx_prof_object_sample_once
ON prof_object_sample(sample_scope, sample_key, sample_kind)
WHERE sample_kind IN ('first_seen', 'first_non_empty');

CREATE INDEX idx_prof_object_sample_key
ON prof_object_sample(sample_scope, sample_key, sample_rank);

CREATE INDEX idx_prof_object_sample_shape
ON prof_object_sample(shape_id, sample_rank);
```

Semantics:

```text
first_seen:
  mandatory first observation for a materialized sample key

first_non_empty:
  first structurally non-empty observation for the same key, if any

priority_sample:
  bounded representative sample selected by deterministic priority
```

`first_seen` and `first_non_empty` are unique per `(sample_scope, sample_key, sample_kind)`.

---

## prof_shape

`prof_shape` stores detailed structural variants.

```sql
CREATE TABLE prof_shape (
    shape_id TEXT PRIMARY KEY,

    canonical_path TEXT NOT NULL,
    site_path TEXT,
    schema_path TEXT NOT NULL,

    field_set_hash TEXT NOT NULL,
    type_set_hash TEXT NOT NULL,

    field_set_json TEXT NOT NULL,
    type_set_json TEXT NOT NULL,

    object_count INTEGER NOT NULL CHECK (object_count >= 0),

    first_seen_document_index INTEGER,
    first_seen_path TEXT
);

CREATE INDEX idx_prof_shape_canonical
ON prof_shape(canonical_path, object_count DESC);

CREATE INDEX idx_prof_shape_site
ON prof_shape(site_path, object_count DESC);

CREATE INDEX idx_prof_shape_schema
ON prof_shape(schema_path, object_count DESC);

CREATE INDEX idx_prof_shape_field_set
ON prof_shape(field_set_hash);

CREATE INDEX idx_prof_shape_type_set
ON prof_shape(type_set_hash);
```

Shape identity grain:

```text
canonical_path + site_path + schema_path + field_set_hash + type_set_hash
```

A single site_path may have multiple prof_shape rows, including when the site represents object elements under a heterogeneous array. The distinguishing keys are field_set_hash and type_set_hash.

prof_shape does not represent array-specific statistics. Array length distribution, element-type distribution, scalar item distribution, and positional semantics are outside v0.1.0.

---

## prof_shape_field

`prof_shape_field` stores shape-specific field slots.

```sql
CREATE TABLE prof_shape_field (
    field_profile_id TEXT PRIMARY KEY,

    shape_id TEXT NOT NULL,
    field_name TEXT NOT NULL,

    observed_type TEXT NOT NULL CHECK (
        observed_type IN ('null','boolean','integer','number','string','object','array','unknown')
    ),

    observed_count INTEGER NOT NULL CHECK (observed_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),

    FOREIGN KEY (shape_id) REFERENCES prof_shape(shape_id)
);

CREATE UNIQUE INDEX idx_prof_shape_field_unique
ON prof_shape_field(shape_id, field_name, observed_type);

CREATE INDEX idx_prof_shape_field_name
ON prof_shape_field(field_name);
```

Field profile identity grain:

```text
shape_id + field_name + observed_type
```

---

## prof_field_summary

`prof_field_summary` stores value-level summary facts for one `field_profile_id`.

```sql
CREATE TABLE prof_field_summary (
    field_profile_id TEXT PRIMARY KEY,

    profiled_count INTEGER NOT NULL CHECK (profiled_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),
    non_null_count INTEGER NOT NULL DEFAULT 0 CHECK (non_null_count >= 0),

    empty_object_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_object_count >= 0),
    empty_array_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_array_count >= 0),

    distinct_count INTEGER CHECK (distinct_count >= 0),

    distinct_count_method TEXT NOT NULL CHECK (
        distinct_count_method IN ('exact', 'approximate', 'unavailable')
    ),

    distinct_algorithm TEXT CHECK (distinct_algorithm IN ('hyperloglog')),
    distinct_error_rate REAL,

    stored_value_count INTEGER NOT NULL DEFAULT 0 CHECK (stored_value_count >= 0),

    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);
```

Special cases:

```text
null only:
  profiled_count = null_count

empty object only:
  profiled_count = empty_object_count
  observed_type = object

empty array only:
  profiled_count = empty_array_count
  observed_type = array
```

`empty_array_count` counts field values whose observed value is an empty array. It is not an array length distribution.

---

## prof_field_value

`prof_field_value` stores selected value facts. It is not necessarily a complete distribution.

```sql
CREATE TABLE prof_field_value (
    field_profile_id TEXT NOT NULL,
    value_hash TEXT NOT NULL,

    value_type TEXT NOT NULL CHECK (
        value_type IN ('null','boolean','integer','number','string','object','array','unknown')
    ),

    value_text TEXT,
    value_text_truncated INTEGER NOT NULL DEFAULT 0 CHECK (value_text_truncated IN (0, 1)),

    count INTEGER CHECK (count >= 0),

    count_method TEXT NOT NULL CHECK (
        count_method IN ('exact', 'approximate', 'sampled', 'unavailable')
    ),

    value_source TEXT NOT NULL CHECK (
        value_source IN ('exact_full', 'exact_selected', 'heavy_hitter', 'sampled')
    ),

    rank INTEGER,

    is_complete_distribution INTEGER NOT NULL DEFAULT 0 CHECK (is_complete_distribution IN (0, 1)),

    PRIMARY KEY (field_profile_id, value_hash, value_source),
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE INDEX idx_prof_field_value_count
ON prof_field_value(field_profile_id, count DESC);

CREATE INDEX idx_prof_field_value_hash
ON prof_field_value(value_hash);
```

A missing row in this table does not necessarily mean a value was never observed. It may mean the value was outside exact storage, heavy hitter candidates, or samples.

---

## prof_field_value_sample

`prof_field_value_sample` stores source-backed value/context samples.

```sql
CREATE TABLE prof_field_value_sample (
    value_sample_id TEXT PRIMARY KEY,

    field_profile_id TEXT NOT NULL,
    value_hash TEXT,

    sample_kind TEXT NOT NULL CHECK (
        sample_kind IN ('first_seen','first_non_empty','priority_sample','heavy_hitter_context')
    ),

    document_index INTEGER,
    source_path TEXT,

    value_json TEXT,
    value_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (value_json_truncated IN (0, 1)),

    parent_object_json TEXT,
    parent_object_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (parent_object_json_truncated IN (0, 1)),

    sample_priority INTEGER,
    sample_rank INTEGER,

    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE INDEX idx_prof_field_value_sample_field
ON prof_field_value_sample(field_profile_id, sample_rank);

CREATE INDEX idx_prof_field_value_sample_hash
ON prof_field_value_sample(value_hash);
```

`sample_kind = 'heavy_hitter_context'` is optional and disabled by default in `v0.1.0-rc.2`.

When present, `heavy_hitter_context` rows must correspond only to final surviving heavy hitter values after Space-Saving finalization. They must not be emitted for transient scan-time candidates.

Default value-context payload limits for rc.2:

```text
value_json_limit_bytes: 1024
parent_object_json_limit_bytes: 1024
priority_sample_limit_per_field_profile: 4
heavy_hitter_context_sample_limit: 0
```


---

## No Views in v0.1.0

`profile-json-refs` does not create SQLite views in v0.1.0.

Physical `prof_*` tables are the output contract.
