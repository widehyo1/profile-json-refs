use std::collections::BTreeSet;

use profile_json_refs::sqlite::schema::{configure_connection, create_indexes, create_schema};
use rusqlite::Connection;

fn names(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect("prepare name query");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query names")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect names")
}

fn table_names(conn: &Connection) -> Vec<String> {
    names(
        conn,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
}

fn view_names(conn: &Connection) -> Vec<String> {
    names(
        conn,
        "SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name",
    )
}

fn column_names(conn: &Connection, table: &str) -> BTreeSet<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("prepare table_info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query columns")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("collect columns")
}

fn schema_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory DB");
    configure_connection(&conn).expect("configure profile DB");
    create_schema(&conn).expect("create profile schema");
    create_indexes(&conn).expect("create profile indexes");
    conn
}

#[test]
fn creates_exactly_approved_profile_tables() {
    let conn = schema_connection();

    assert_eq!(
        table_names(&conn),
        vec![
            "prof_field_summary",
            "prof_field_value",
            "prof_field_value_sample",
            "prof_object_sample",
            "prof_shape",
            "prof_shape_field",
            "prof_source_summary",
        ]
    );
}

#[test]
fn creates_no_views_or_forbidden_tables() {
    let conn = schema_connection();
    let tables = table_names(&conn);

    assert!(view_names(&conn).is_empty());
    for forbidden in [
        "prof_path_sample",
        "prof_shape_sample",
        "prof_run",
        "prof_manifest",
        "prof_algorithm",
        "prof_warning",
    ] {
        assert!(!tables.iter().any(|table| table == forbidden));
    }
    assert!(!tables.iter().any(|table| table.starts_with("prof_array_")));
}

#[test]
fn field_summary_includes_empty_string_count() {
    let conn = schema_connection();
    let columns = column_names(&conn, "prof_field_summary");

    assert!(columns.contains("empty_string_count"));
}

#[test]
fn object_sample_table_exists_but_path_and_shape_sample_tables_do_not() {
    let conn = schema_connection();
    let tables = table_names(&conn);

    assert!(tables.iter().any(|table| table == "prof_object_sample"));
    assert!(!tables.iter().any(|table| table == "prof_path_sample"));
    assert!(!tables.iter().any(|table| table == "prof_shape_sample"));
}

#[test]
fn first_seen_object_sample_is_unique_per_scope_key_kind() {
    let conn = schema_connection();

    conn.execute(
        "\
        INSERT INTO prof_object_sample (
            object_sample_id, sample_scope, sample_key, canonical_path, sample_kind, sample_json
        ) VALUES (?1, 'canonical_path', '$', '$', 'first_seen', '{}')
        ",
        ["sample-1"],
    )
    .expect("insert first sample");

    let err = conn
        .execute(
            "\
            INSERT INTO prof_object_sample (
                object_sample_id, sample_scope, sample_key, canonical_path, sample_kind, sample_json
            ) VALUES (?1, 'canonical_path', '$', '$', 'first_seen', '{}')
            ",
            ["sample-2"],
        )
        .expect_err("duplicate first_seen sample should violate unique index");

    assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
}

#[test]
fn foreign_keys_are_enabled_by_connection_configuration() {
    let conn = schema_connection();

    let enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("read foreign key pragma");

    assert_eq!(enabled, 1);
}
