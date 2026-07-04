use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use profile_json_refs::config::{
    FlushConfig, InputFormat, ProfileConfig, SamplingConfig, ValueProfileConfig,
};
use profile_json_refs::field::accumulator::ShapeFieldRow;
use profile_json_refs::field::summary::{DistinctCountMethod, FieldSummary};
use profile_json_refs::shape::accumulator::ShapeRow;
use profile_json_refs::shape::sample::{ObjectSampleKind, ObjectSampleRow, SampleScope};
use profile_json_refs::sqlite::writer::{ProfileChunk, ProfileWriter, SourceCounters};
use profile_json_refs::util::json_type::JsonType;
use profile_json_refs::value::exact_counter::{CountMethod, FieldValueRow, ValueSource};
use profile_json_refs::value::sample::{ValueSampleKind, ValueSampleRow};
use profile_json_refs::{SourceSummary, run};
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

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "profile-json-refs-writer-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn create_refs_db(path: &Path) {
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
        ) VALUES (?1, '$', 'object', 1, 1, 0)",
        ["refs/root.json"],
    )
    .expect("insert presence shape limits");
}

fn test_config(out: &Path) -> ProfileConfig {
    let input_file = out.with_file_name("input.json");
    let refs_sqlite = out.with_file_name("refs.sqlite");
    fs::write(&input_file, "{}").expect("write input fixture");
    Connection::open(&refs_sqlite).expect("create refs placeholder");

    ProfileConfig {
        input_file,
        refs_sqlite,
        out_sqlite: out.to_path_buf(),
        input_format: InputFormat::Json,
        quiet: false,
        perf_log: false,
        sampling: SamplingConfig {
            canonical_priority_limit: 1,
            site_priority_limit: 1,
            field_set_priority_limit: 1,
            type_set_priority_limit: 1,
            value_priority_limit_per_field_profile: 1,
            ..SamplingConfig::default()
        },
        value_profile: ValueProfileConfig::default(),
        flush: FlushConfig::default(),
    }
}

fn run_config(input: &Path, refs: &Path, out: &Path) -> ProfileConfig {
    ProfileConfig {
        input_file: input.to_path_buf(),
        refs_sqlite: refs.to_path_buf(),
        out_sqlite: out.to_path_buf(),
        input_format: InputFormat::Json,
        quiet: false,
        perf_log: false,
        sampling: SamplingConfig {
            canonical_priority_limit: 1,
            site_priority_limit: 1,
            field_set_priority_limit: 1,
            type_set_priority_limit: 1,
            value_priority_limit_per_field_profile: 1,
            ..SamplingConfig::default()
        },
        value_profile: ValueProfileConfig::default(),
        flush: FlushConfig::default(),
    }
}

fn shape_row() -> ShapeRow {
    ShapeRow {
        shape_id: "shape-1".to_string(),
        canonical_path: "$".to_string(),
        site_path: Some("$".to_string()),
        schema_path: "refs/root.json".to_string(),
        field_set_hash: "field-hash".to_string(),
        type_set_hash: "type-hash".to_string(),
        field_set_json: r#"["id"]"#.to_string(),
        type_set_json: r#"[["id","integer"]]"#.to_string(),
        object_count: 2,
        first_seen_document_index: Some(0),
        first_seen_path: Some("$".to_string()),
    }
}

fn shape_field_row() -> ShapeFieldRow {
    ShapeFieldRow {
        field_profile_id: "field-1".to_string(),
        shape_id: "shape-1".to_string(),
        field_name: "id".to_string(),
        observed_type: JsonType::Integer,
        observed_count: 2,
        null_count: 0,
    }
}

fn field_summary() -> FieldSummary {
    FieldSummary {
        field_profile_id: "field-1".to_string(),
        profiled_count: 2,
        null_count: 0,
        non_null_count: 2,
        empty_object_count: 0,
        empty_array_count: 0,
        empty_string_count: 0,
        distinct_count: Some(2),
        distinct_count_method: DistinctCountMethod::Exact,
        distinct_algorithm: None,
        distinct_error_rate: None,
        stored_value_count: 1,
    }
}

fn field_value_row() -> FieldValueRow {
    FieldValueRow {
        field_profile_id: "field-1".to_string(),
        value_hash: "value-1".to_string(),
        value_type: JsonType::Integer,
        value_text: Some("1".to_string()),
        value_text_truncated: false,
        count: Some(2),
        count_method: CountMethod::Exact,
        value_source: ValueSource::ExactFull,
        rank: Some(1),
        is_complete_distribution: true,
    }
}

fn object_sample_row(id: &str, kind: ObjectSampleKind, priority: Option<u64>) -> ObjectSampleRow {
    ObjectSampleRow {
        object_sample_id: id.to_string(),
        sample_scope: SampleScope::CanonicalPath,
        sample_key: "$".to_string(),
        canonical_path: "$".to_string(),
        site_path: Some("$".to_string()),
        schema_path: Some("refs/root.json".to_string()),
        field_set_hash: Some("field-hash".to_string()),
        type_set_hash: Some("type-hash".to_string()),
        shape_id: Some("shape-1".to_string()),
        sample_kind: kind,
        document_index: priority.unwrap_or(0),
        source_path: "$".to_string(),
        sample_json: r#"{"id":1}"#.to_string(),
        sample_json_truncated: false,
        sample_is_empty_object: false,
        sample_is_empty_array: false,
        sample_priority: priority,
        sample_rank: priority.map(|_| 1),
    }
}

fn value_sample_row(id: &str) -> ValueSampleRow {
    ValueSampleRow {
        value_sample_id: id.to_string(),
        field_profile_id: "field-1".to_string(),
        value_hash: Some("value-1".to_string()),
        sample_kind: ValueSampleKind::FirstSeen,
        document_index: 0,
        source_path: "$.id".to_string(),
        value_json: Some("1".to_string()),
        value_json_truncated: false,
        parent_object_json: Some(r#"{"id":1}"#.to_string()),
        parent_object_json_truncated: false,
        sample_priority: None,
        sample_rank: None,
    }
}

fn table_count(conn: &Connection, table: &str) -> u64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get::<_, u64>(0)
    })
    .expect("query table count")
}

#[test]
fn flush_writes_all_profile_fact_tables() {
    let dir = unique_temp_dir("all-tables");
    let out = dir.join("profile.sqlite");
    let config = test_config(&out);
    let mut writer = ProfileWriter::open(&out, &config).expect("open writer");

    writer
        .flush_chunk(ProfileChunk {
            shapes: vec![shape_row()],
            shape_fields: vec![shape_field_row()],
            object_samples: vec![object_sample_row(
                "sample-first",
                ObjectSampleKind::FirstSeen,
                None,
            )],
            field_summaries: vec![field_summary()],
            field_values: vec![field_value_row()],
            value_samples: vec![value_sample_row("value-sample-first")],
        })
        .expect("flush chunk");

    let conn = writer.connection();
    assert_eq!(table_count(conn, "prof_shape"), 1);
    assert_eq!(table_count(conn, "prof_shape_field"), 1);
    assert_eq!(table_count(conn, "prof_object_sample"), 1);
    assert_eq!(table_count(conn, "prof_field_summary"), 1);
    assert_eq!(table_count(conn, "prof_field_value"), 1);
    assert_eq!(table_count(conn, "prof_field_value_sample"), 1);
}

#[test]
fn first_seen_object_samples_use_insert_or_ignore_across_flushes() {
    let dir = unique_temp_dir("first-seen-ignore");
    let out = dir.join("profile.sqlite");
    let config = test_config(&out);
    let mut writer = ProfileWriter::open(&out, &config).expect("open writer");

    writer
        .flush_chunk(ProfileChunk {
            shapes: vec![shape_row()],
            shape_fields: Vec::new(),
            object_samples: vec![object_sample_row(
                "sample-first-1",
                ObjectSampleKind::FirstSeen,
                None,
            )],
            field_summaries: Vec::new(),
            field_values: Vec::new(),
            value_samples: Vec::new(),
        })
        .expect("first flush");
    writer
        .flush_chunk(ProfileChunk {
            shapes: Vec::new(),
            shape_fields: Vec::new(),
            object_samples: vec![object_sample_row(
                "sample-first-2",
                ObjectSampleKind::FirstSeen,
                None,
            )],
            field_summaries: Vec::new(),
            field_values: Vec::new(),
            value_samples: Vec::new(),
        })
        .expect("second flush");

    assert_eq!(table_count(writer.connection(), "prof_object_sample"), 1);
}

#[test]
fn priority_object_samples_are_pruned_and_ranked_after_flush() {
    let dir = unique_temp_dir("priority-prune");
    let out = dir.join("profile.sqlite");
    let config = test_config(&out);
    let mut writer = ProfileWriter::open(&out, &config).expect("open writer");

    writer
        .flush_chunk(ProfileChunk {
            shapes: vec![shape_row()],
            shape_fields: Vec::new(),
            object_samples: vec![
                object_sample_row("priority-worse", ObjectSampleKind::PrioritySample, Some(20)),
                object_sample_row(
                    "priority-better",
                    ObjectSampleKind::PrioritySample,
                    Some(10),
                ),
            ],
            field_summaries: Vec::new(),
            field_values: Vec::new(),
            value_samples: Vec::new(),
        })
        .expect("flush priority samples");

    let kept: (String, u32) = writer
        .connection()
        .query_row(
            "SELECT object_sample_id, sample_rank FROM prof_object_sample WHERE sample_kind = 'priority_sample'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query kept priority sample");

    assert_eq!(kept, ("priority-better".to_string(), 1));
}

#[test]
fn source_summary_is_derived_from_persisted_profile_counts() {
    let dir = unique_temp_dir("summary");
    let out = dir.join("profile.sqlite");
    let config = test_config(&out);
    let mut writer = ProfileWriter::open(&out, &config).expect("open writer");
    writer
        .flush_chunk(ProfileChunk {
            shapes: vec![shape_row()],
            shape_fields: vec![shape_field_row()],
            object_samples: Vec::new(),
            field_summaries: vec![field_summary()],
            field_values: vec![field_value_row()],
            value_samples: Vec::new(),
        })
        .expect("flush chunk");

    let summary = writer
        .write_source_summary(
            "json",
            SourceCounters {
                total_document_count: 1,
                total_object_count: 2,
                total_array_count: 3,
                total_scalar_count: 4,
            },
        )
        .expect("write source summary");

    assert_eq!(summary.total_document_count, 1);
    assert_eq!(summary.total_object_count, 2);
    assert_eq!(summary.total_array_count, 3);
    assert_eq!(summary.total_scalar_count, 4);
    assert_eq!(summary.total_canonical_path_count, 1);
    assert_eq!(summary.total_site_path_count, 1);
    assert_eq!(summary.total_shape_count, 1);
    assert_eq!(summary.total_field_profile_count, 1);
    assert_eq!(summary.total_stored_value_count, 1);

    assert_eq!(table_count(writer.connection(), "prof_source_summary"), 1);
}

#[test]
fn run_writes_usable_profile_sqlite_from_json_input() {
    let dir = unique_temp_dir("run-pipeline");
    let input = dir.join("input.json");
    let refs = dir.join("refs.sqlite");
    let out = dir.join("profile.sqlite");
    fs::write(&input, r#"{"id":1,"name":"Ada"}"#).expect("write input");
    create_refs_db(&refs);

    let report = run(run_config(&input, &refs, &out)).expect("run profile");

    assert_eq!(
        report.summary,
        SourceSummary {
            total_document_count: 1,
            total_object_count: 1,
            total_array_count: 0,
            total_scalar_count: 2,
            total_canonical_path_count: 1,
            total_site_path_count: 1,
            total_shape_count: 1,
            total_field_profile_count: 2,
            total_stored_value_count: 2,
        }
    );
    assert!(out.is_file());

    let conn = Connection::open(out).expect("open output profile");
    assert_eq!(table_count(&conn, "prof_source_summary"), 1);
    assert_eq!(table_count(&conn, "prof_shape"), 1);
    assert_eq!(table_count(&conn, "prof_shape_field"), 2);
    assert_eq!(table_count(&conn, "prof_field_value"), 2);
}
