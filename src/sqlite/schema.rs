use rusqlite::Connection;

pub fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}

pub fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PROFILE_SCHEMA_SQL)
}

pub fn create_indexes(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PROFILE_INDEX_SQL)
}

pub const PROFILE_SCHEMA_SQL: &str = r#"
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
"#;

pub const PROFILE_INDEX_SQL: &str = r#"
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
"#;
