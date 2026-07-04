use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use profile_json_refs::refs::contract::{REQUIRED_TABLES, validate_required_tables};
use profile_json_refs::refs::resolver::RefsResolver;
use profile_json_refs::refs::site::{RefsIndex, SiteContext};
use profile_json_refs::refs::sqlite::{
    W_REFS_PRESENCE_SHAPES_TRUNCATED, load_refs_index, load_refs_index_from_path,
};
use profile_json_refs::scan::path::SourcePath;
use rusqlite::Connection;

const REFS_DDL: &str = r#"
CREATE TABLE schema_paths (
    schema_path TEXT NOT NULL,
    object_path TEXT PRIMARY KEY
);

CREATE TABLE array_index_refs (
    array_path TEXT NOT NULL,
    array_index_path TEXT NOT NULL,
    schema_path TEXT NOT NULL,
    PRIMARY KEY (array_path, array_index_path)
);

CREATE TABLE schema_definitions (
    schema_path TEXT PRIMARY KEY,
    schema_kind TEXT NOT NULL,
    schema_json TEXT NOT NULL
);

CREATE TABLE schema_object_counts (
    schema_path TEXT PRIMARY KEY,
    object_count INTEGER NOT NULL CHECK (object_count > 0)
);

CREATE TABLE schema_field_counts (
    schema_path TEXT NOT NULL,
    field_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    field_count INTEGER NOT NULL CHECK (field_count > 0),
    PRIMARY KEY (schema_path, field_name, field_type)
);

CREATE TABLE schema_site_counts (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    object_count INTEGER NOT NULL CHECK (object_count > 0),
    PRIMARY KEY (schema_path, site_path, site_kind)
);

CREATE TABLE schema_site_field_counts (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    field_name TEXT NOT NULL,
    schema_field_type TEXT NOT NULL,
    present_count INTEGER NOT NULL CHECK (present_count >= 0),
    missing_count INTEGER NOT NULL CHECK (missing_count >= 0),
    PRIMARY KEY (schema_path, site_path, site_kind, field_name)
);

CREATE TABLE schema_site_presence_shapes (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    present_fields_hash TEXT NOT NULL,
    present_fields_json TEXT NOT NULL,
    missing_fields_json TEXT NOT NULL,
    object_count INTEGER NOT NULL CHECK (object_count > 0),
    first_array_index_path TEXT,
    PRIMARY KEY (schema_path, site_path, site_kind, present_fields_hash)
);

CREATE TABLE schema_site_presence_shape_limits (
    schema_path TEXT NOT NULL,
    site_path TEXT NOT NULL,
    site_kind TEXT NOT NULL CHECK (site_kind IN ('object', 'array_item', 'root_collection')),
    observed_shape_count INTEGER NOT NULL CHECK (observed_shape_count >= 0),
    stored_shape_count INTEGER NOT NULL CHECK (stored_shape_count >= 0),
    truncated INTEGER NOT NULL CHECK (truncated IN (0, 1)),
    PRIMARY KEY (schema_path, site_path, site_kind)
);
"#;

fn refs_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open refs fixture");
    conn.execute_batch(REFS_DDL).expect("create refs fixture");
    conn
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "profile-json-refs-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn refs_db_at(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open refs fixture path");
    conn.execute_batch(REFS_DDL).expect("create refs fixture");
    conn
}

fn insert_minimal_refs_rows(conn: &Connection, truncated: bool) {
    conn.execute(
        "INSERT INTO schema_paths(schema_path, object_path) VALUES (?1, ?2)",
        ("refs/root.json", "$"),
    )
    .expect("insert schema path");
    conn.execute(
        "INSERT INTO schema_definitions(schema_path, schema_kind, schema_json) VALUES (?1, 'object', ?2)",
        ("refs/root.json", r#"{"id":"number","name":"string?"}"#),
    )
    .expect("insert schema definition");
    conn.execute(
        "INSERT INTO schema_object_counts(schema_path, object_count) VALUES (?1, 2)",
        ["refs/root.json"],
    )
    .expect("insert object count");
    conn.execute(
        "INSERT INTO schema_field_counts(schema_path, field_name, field_type, field_count) VALUES (?1, 'id', 'number', 2)",
        ["refs/root.json"],
    )
    .expect("insert field count");
    conn.execute(
        "INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count) VALUES (?1, ?2, 'object', 2)",
        ("refs/root.json", "$"),
    )
    .expect("insert site count");
    conn.execute(
        "INSERT INTO schema_site_field_counts(
            schema_path, site_path, site_kind, field_name, schema_field_type, present_count, missing_count
        ) VALUES (?1, ?2, 'object', 'id', 'number', 2, 0)",
        ("refs/root.json", "$"),
    )
    .expect("insert site field count");
    conn.execute(
        "INSERT INTO schema_site_presence_shapes(
            schema_path, site_path, site_kind, present_fields_hash, present_fields_json,
            missing_fields_json, object_count, first_array_index_path
        ) VALUES (?1, ?2, 'object', 'hash-id', ?3, ?4, 2, NULL)",
        ("refs/root.json", "$", r#"["id"]"#, r#"[]"#),
    )
    .expect("insert presence shape");
    conn.execute(
        "INSERT INTO schema_site_presence_shape_limits(
            schema_path, site_path, site_kind, observed_shape_count, stored_shape_count, truncated
        ) VALUES (?1, ?2, 'object', 2, 1, ?3)",
        ("refs/root.json", "$", i64::from(truncated)),
    )
    .expect("insert presence shape limits");
}

#[test]
fn load_refs_index_from_path_opens_sqlite_file() {
    let dir = unique_temp_dir("refs-path");
    let refs_path = dir.join("schemas.sqlite");
    let conn = refs_db_at(&refs_path);
    insert_minimal_refs_rows(&conn, false);
    drop(conn);

    let loaded = load_refs_index_from_path(&refs_path).expect("load refs from path");

    assert!(loaded.warnings.is_empty());
    assert_eq!(
        loaded
            .index
            .schema_by_canonical
            .get("$")
            .map(String::as_str),
        Some("refs/root.json")
    );
}

#[test]
fn fixture_with_required_tables_validates() {
    let conn = refs_db();

    validate_required_tables(&conn).expect("required refs tables should validate");
    assert_eq!(REQUIRED_TABLES.len(), 9);
}

#[test]
fn missing_schema_site_counts_fails_validation() {
    let conn = refs_db();
    conn.execute("DROP TABLE schema_site_counts", [])
        .expect("drop schema_site_counts");

    let err = validate_required_tables(&conn).expect_err("missing table should fail");

    assert!(
        err.to_string()
            .contains("refs database is missing required table: schema_site_counts")
    );
}

#[test]
fn missing_schema_site_presence_shapes_fails_validation() {
    let conn = refs_db();
    conn.execute("DROP TABLE schema_site_presence_shapes", [])
        .expect("drop schema_site_presence_shapes");

    let err = validate_required_tables(&conn).expect_err("missing table should fail");

    assert!(
        err.to_string()
            .contains("refs database is missing required table: schema_site_presence_shapes")
    );
}

#[test]
fn load_refs_index_maps_schema_site_and_presence_shape_seeds() {
    let conn = refs_db();
    insert_minimal_refs_rows(&conn, false);

    let loaded = load_refs_index(&conn).expect("load refs index");

    assert!(loaded.warnings.is_empty());
    assert!(!loaded.index.presence_shape_truncated);
    assert_eq!(
        loaded
            .index
            .schema_by_canonical
            .get("$")
            .map(String::as_str),
        Some("refs/root.json")
    );
    let site = loaded
        .index
        .site_by_source_path
        .get("$")
        .expect("site context for root source path");
    assert_eq!(site.canonical_path, "$");
    assert_eq!(site.site_path.as_deref(), Some("$"));
    assert_eq!(site.schema_path, "refs/root.json");
    assert_eq!(loaded.index.presence_shape_seeds.len(), 1);
    assert_eq!(loaded.index.presence_shape_seeds[0].field_names, vec!["id"]);
    assert!(!loaded.index.presence_shape_seeds[0].truncated);
}

#[test]
fn truncated_presence_shape_fixture_warns_but_does_not_fail() {
    let conn = refs_db();
    insert_minimal_refs_rows(&conn, true);

    let loaded = load_refs_index(&conn).expect("truncated shapes should not fail");

    assert!(loaded.index.presence_shape_truncated);
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(loaded.warnings[0].code, W_REFS_PRESENCE_SHAPES_TRUNCATED);
    assert!(loaded.index.presence_shape_seeds[0].truncated);
}

#[test]
fn resolver_uses_refs_context_and_falls_back_to_canonical_guess() {
    let mut index = RefsIndex::default();
    index.site_by_source_path.insert(
        "$.items[0]".to_string(),
        SiteContext {
            canonical_path: "$.items[]".to_string(),
            site_path: Some("$.items[0]".to_string()),
            schema_path: "refs/root/items/item.json".to_string(),
        },
    );
    let resolver = RefsResolver::new(index);

    let mut resolved_path = SourcePath::root();
    resolved_path.push_field("items");
    resolved_path.push_index(0);
    let resolved = resolver.resolve_object(&resolved_path);
    assert!(resolved.resolved);
    assert_eq!(resolved.canonical_path, "$.items[]");
    assert_eq!(resolved.site_path.as_deref(), Some("$.items[0]"));
    assert_eq!(resolved.schema_path, "refs/root/items/item.json");

    let mut fallback_path = SourcePath::root();
    fallback_path.push_field("items");
    fallback_path.push_index(99);
    let fallback = resolver.resolve_object(&fallback_path);
    assert!(!fallback.resolved);
    assert_eq!(fallback.canonical_path, "$.items[]");
    assert_eq!(fallback.site_path.as_deref(), Some("$.items[99]"));
    assert_eq!(fallback.schema_path, "unknown");
}
