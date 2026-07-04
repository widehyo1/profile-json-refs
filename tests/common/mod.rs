use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

pub const REFS_DDL: &str = r#"
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

pub struct FixturePaths {
    pub input: PathBuf,
    pub refs: PathBuf,
    pub out: PathBuf,
}

pub fn unique_temp_dir(name: &str) -> PathBuf {
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

pub fn basic_fixture(name: &str, input_json: &str, truncated_refs: bool) -> FixturePaths {
    let dir = unique_temp_dir(name);
    let input = dir.join("input.json");
    let refs = dir.join("refs.sqlite");
    let out = dir.join("profile.sqlite");
    fs::write(&input, input_json).expect("write input fixture");
    create_refs_db(&refs, truncated_refs);
    FixturePaths { input, refs, out }
}

pub fn create_refs_db(path: &Path, truncated: bool) {
    let conn = Connection::open(path).expect("open refs db");
    conn.execute_batch(REFS_DDL).expect("create refs schema");
    conn.execute(
        "INSERT INTO schema_paths(schema_path, object_path) VALUES (?1, ?2)",
        ("refs/root.json", "$"),
    )
    .expect("insert schema path");
    conn.execute(
        "INSERT INTO schema_definitions(schema_path, schema_kind, schema_json) VALUES (?1, 'object', ?2)",
        ("refs/root.json", "{}"),
    )
    .expect("insert schema definition");
    conn.execute(
        "INSERT INTO schema_object_counts(schema_path, object_count) VALUES (?1, 1)",
        ["refs/root.json"],
    )
    .expect("insert object count");
    conn.execute(
        "INSERT INTO schema_field_counts(schema_path, field_name, field_type, field_count) VALUES (?1, 'id', 'number', 1)",
        ["refs/root.json"],
    )
    .expect("insert field count");
    conn.execute(
        "INSERT INTO schema_site_counts(schema_path, site_path, site_kind, object_count) VALUES (?1, '$', 'object', 1)",
        ["refs/root.json"],
    )
    .expect("insert site count");
    conn.execute(
        "INSERT INTO schema_site_field_counts(
            schema_path, site_path, site_kind, field_name, schema_field_type, present_count, missing_count
        ) VALUES (?1, '$', 'object', 'id', 'number', 1, 0)",
        ["refs/root.json"],
    )
    .expect("insert site field count");
    conn.execute(
        "INSERT INTO schema_site_presence_shapes(
            schema_path, site_path, site_kind, present_fields_hash, present_fields_json,
            missing_fields_json, object_count, first_array_index_path
        ) VALUES (?1, '$', 'object', 'hash-id', ?2, ?3, 1, NULL)",
        ("refs/root.json", r#"["id"]"#, r#"[]"#),
    )
    .expect("insert presence shape");
    conn.execute(
        "INSERT INTO schema_site_presence_shape_limits(
            schema_path, site_path, site_kind, observed_shape_count, stored_shape_count, truncated
        ) VALUES (?1, '$', 'object', 1, 1, ?2)",
        ("refs/root.json", i64::from(truncated)),
    )
    .expect("insert presence shape limits");
}

pub fn run_profile(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_profile-json-refs"))
        .args(args)
        .output()
        .expect("run profile-json-refs")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is utf8")
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is utf8")
}
