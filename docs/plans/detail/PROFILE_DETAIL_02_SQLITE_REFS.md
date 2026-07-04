# Profile Detail 02: SQLite Schema and Refs Adapter

Covers:

```text
Phase 2: SQLite schema writer
Phase 3: refs adapter
```

---

## 1. Target Files

```text
src/sqlite/mod.rs
src/sqlite/schema.rs
src/sqlite/writer.rs
src/refs/mod.rs
src/refs/sqlite.rs
src/refs/contract.rs
src/refs/site.rs
src/refs/resolver.rs
tests/sqlite_schema.rs
tests/refs_contract.rs
fixtures/refs/
```

---

## 2. Approved Physical Tables

v0.1.0 creates exactly:

```text
prof_source_summary
prof_object_sample
prof_shape
prof_shape_field
prof_field_summary
prof_field_value
prof_field_value_sample
```

Forbidden:

```text
prof_path_sample
prof_shape_sample
prof_run
prof_manifest
prof_algorithm
prof_warning
prof_array_*
views
```

---

## 3. SQLite Schema Creation API

`src/sqlite/schema.rs`:

```rust
use rusqlite::Connection;

pub fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PROFILE_SCHEMA_SQL)
}

pub fn create_indexes(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PROFILE_INDEX_SQL)
}

pub fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}
```

For bulk loading, it is acceptable to create most indexes after inserts in Phase 9. In Phase 2 tests, DDL and indexes can be created together.

---

## 4. DDL

Use integer counters as SQLite `INTEGER`. Rust should use `u64` internally and convert carefully.

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

CREATE TABLE prof_object_sample (
    object_sample_id TEXT PRIMARY KEY,
    sample_scope TEXT NOT NULL CHECK (sample_scope IN ('canonical_path', 'site_path', 'field_set', 'type_set')),
    sample_key TEXT NOT NULL,
    canonical_path TEXT NOT NULL,
    site_path TEXT,
    schema_path TEXT,
    field_set_hash TEXT,
    type_set_hash TEXT,
    shape_id TEXT,
    sample_kind TEXT NOT NULL CHECK (sample_kind IN ('first_seen', 'first_non_empty', 'priority_sample')),
    document_index INTEGER,
    source_path TEXT,
    sample_json TEXT NOT NULL,
    sample_json_truncated INTEGER NOT NULL DEFAULT 0 CHECK (sample_json_truncated IN (0, 1)),
    sample_is_empty_object INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_object IN (0, 1)),
    sample_is_empty_array INTEGER NOT NULL DEFAULT 0 CHECK (sample_is_empty_array IN (0, 1)),
    sample_priority INTEGER,
    sample_rank INTEGER
);

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

CREATE TABLE prof_shape_field (
    field_profile_id TEXT PRIMARY KEY,
    shape_id TEXT NOT NULL,
    field_name TEXT NOT NULL,
    observed_type TEXT NOT NULL CHECK (observed_type IN ('null','boolean','integer','number','string','object','array','unknown')),
    observed_count INTEGER NOT NULL CHECK (observed_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),
    FOREIGN KEY (shape_id) REFERENCES prof_shape(shape_id)
);

CREATE TABLE prof_field_summary (
    field_profile_id TEXT PRIMARY KEY,
    profiled_count INTEGER NOT NULL CHECK (profiled_count >= 0),
    null_count INTEGER NOT NULL DEFAULT 0 CHECK (null_count >= 0),
    non_null_count INTEGER NOT NULL DEFAULT 0 CHECK (non_null_count >= 0),
    empty_object_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_object_count >= 0),
    empty_array_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_array_count >= 0),
    empty_string_count INTEGER NOT NULL DEFAULT 0 CHECK (empty_string_count >= 0),
    distinct_count INTEGER CHECK (distinct_count >= 0),
    distinct_count_method TEXT NOT NULL CHECK (distinct_count_method IN ('exact','approximate','unavailable')),
    distinct_algorithm TEXT CHECK (distinct_algorithm IN ('hyperloglog')),
    distinct_error_rate REAL,
    stored_value_count INTEGER NOT NULL DEFAULT 0 CHECK (stored_value_count >= 0),
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE TABLE prof_field_value (
    field_profile_id TEXT NOT NULL,
    value_hash TEXT NOT NULL,
    value_type TEXT NOT NULL CHECK (value_type IN ('null','boolean','integer','number','string','object','array','unknown')),
    value_text TEXT,
    value_text_truncated INTEGER NOT NULL DEFAULT 0 CHECK (value_text_truncated IN (0, 1)),
    count INTEGER CHECK (count >= 0),
    count_method TEXT NOT NULL CHECK (count_method IN ('exact','approximate','sampled','unavailable')),
    value_source TEXT NOT NULL CHECK (value_source IN ('exact_full','exact_selected','heavy_hitter','sampled')),
    rank INTEGER,
    is_complete_distribution INTEGER NOT NULL DEFAULT 0 CHECK (is_complete_distribution IN (0, 1)),
    PRIMARY KEY (field_profile_id, value_hash, value_source),
    FOREIGN KEY (field_profile_id) REFERENCES prof_shape_field(field_profile_id)
);

CREATE TABLE prof_field_value_sample (
    value_sample_id TEXT PRIMARY KEY,
    field_profile_id TEXT NOT NULL,
    value_hash TEXT,
    sample_kind TEXT NOT NULL CHECK (sample_kind IN ('first_seen','first_non_empty','priority_sample','heavy_hitter_context')),
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
```

Indexes:

```sql
CREATE UNIQUE INDEX idx_prof_object_sample_once
ON prof_object_sample(sample_scope, sample_key, sample_kind)
WHERE sample_kind IN ('first_seen', 'first_non_empty');

CREATE INDEX idx_prof_object_sample_key
ON prof_object_sample(sample_scope, sample_key, sample_rank);

CREATE INDEX idx_prof_object_sample_shape
ON prof_object_sample(shape_id, sample_rank);

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

CREATE UNIQUE INDEX idx_prof_shape_field_unique
ON prof_shape_field(shape_id, field_name, observed_type);

CREATE INDEX idx_prof_shape_field_name
ON prof_shape_field(field_name);

CREATE INDEX idx_prof_field_value_count
ON prof_field_value(field_profile_id, count DESC);

CREATE INDEX idx_prof_field_value_hash
ON prof_field_value(value_hash);

CREATE INDEX idx_prof_field_value_sample_field
ON prof_field_value_sample(field_profile_id, sample_rank);

CREATE INDEX idx_prof_field_value_sample_hash
ON prof_field_value_sample(value_hash);
```

`heavy_hitter_context` is an allowed `sample_kind`, but `v0.1.0-rc.2` disables it by default. The schema allows future opt-in context rows without changing the physical table contract.


---

## 5. Refs Contract

Required upstream tables:

```text
schema_paths
array_index_refs
schema_definitions
schema_object_counts
schema_field_counts
schema_site_counts
schema_site_field_counts
schema_site_presence_shapes
schema_site_presence_shape_limits
```

`src/refs/contract.rs`:

```rust
pub const REQUIRED_TABLES: &[&str] = &[
    "schema_paths",
    "array_index_refs",
    "schema_definitions",
    "schema_object_counts",
    "schema_field_counts",
    "schema_site_counts",
    "schema_site_field_counts",
    "schema_site_presence_shapes",
    "schema_site_presence_shape_limits",
];

pub fn validate_required_tables(conn: &rusqlite::Connection) -> crate::error::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1"
    )?;

    for table in REQUIRED_TABLES {
        let found: rusqlite::Result<String> = stmt.query_row([table], |row| row.get(0));
        if found.is_err() {
            return Err(crate::error::ProfileError::InvalidConfig(
                format!("refs database is missing required table: {table}")
            ));
        }
    }

    Ok(())
}
```

Use a dedicated error variant later if desired.

---

## 6. Refs Adapter Data Types

`src/refs/site.rs`:

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SiteContext {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
}

#[derive(Debug, Clone)]
pub struct SitePresenceShapeSeed {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_names: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct RefsIndex {
    pub schema_by_canonical: HashMap<String, String>,
    pub site_by_source_path: HashMap<String, SiteContext>,
    pub presence_shape_truncated: bool,
}
```

The exact mapping columns depend on the upstream refs DB contract. The adapter should isolate all SQL in `src/refs/sqlite.rs`.

Do not let scanner modules issue raw refs SQL directly.

---

## 7. Resolver API

`src/refs/resolver.rs`:

```rust
use crate::scan::path::SourcePath;

pub struct RefsResolver {
    index: crate::refs::site::RefsIndex,
}

impl RefsResolver {
    pub fn new(index: crate::refs::site::RefsIndex) -> Self {
        Self { index }
    }

    pub fn resolve_object(&self, path: &SourcePath) -> ResolvedObjectContext {
        if let Some(site) = self.index.site_by_source_path.get(path.as_str()) {
            return ResolvedObjectContext {
                canonical_path: site.canonical_path.clone(),
                site_path: site.site_path.clone(),
                schema_path: site.schema_path.clone(),
                resolved: true,
            };
        }

        ResolvedObjectContext {
            canonical_path: path.to_canonical_guess(),
            site_path: Some(path.as_str().to_string()),
            schema_path: "unknown".to_string(),
            resolved: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedObjectContext {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub resolved: bool,
}
```

Fallback behavior should warn once per category, not once per occurrence.

---

## 8. Truncated Presence Shapes

If `schema_site_presence_shape_limits` says a site was truncated:

```text
- generation continues
- stderr warning is emitted
- source scan may compute observed profile shapes
- implementation must not claim upstream stored every presence-shape identity
```

Warning code:

```text
W_REFS_PRESENCE_SHAPES_TRUNCATED
```

This warning is not stored in SQLite.

---

## 9. Phase 2 Tests

`tests/sqlite_schema.rs`:

```text
- create empty profile DB
- assert approved table names exactly
- assert no views
- assert no forbidden prof_* tables
- assert prof_field_summary has empty_string_count
- assert prof_object_sample exists
- assert prof_path_sample does not exist
- assert prof_shape_sample does not exist
- assert no prof_array_* table exists
```

SQL helper:

```sql
SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name;
SELECT name FROM sqlite_master WHERE type = 'view';
```

---

## 10. Phase 3 Tests

`tests/refs_contract.rs`:

```text
- fixture refs DB with required tables validates
- missing schema_site_counts fails
- missing schema_site_presence_shapes fails
- truncated presence-shape fixture warns but does not fail
```

---

## 11. Commits

Phase 2:

```bash
git add src/sqlite tests/sqlite_schema.rs fixtures/refs
git commit -m "feat(sqlite): create profile fact schema"
```

Phase 3:

```bash
git add src/refs tests/refs_contract.rs fixtures/refs
git commit -m "feat(refs): load upstream refs contract"
```
