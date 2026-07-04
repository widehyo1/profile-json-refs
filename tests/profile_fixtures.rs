mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{run_profile, stderr, stdout, unique_temp_dir};
use rusqlite::Connection;

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(relative)
}

fn script_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(relative)
}

fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path).expect("script metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("mark script executable");
}

fn create_refs_from_fixture(path: &Path) {
    let sql =
        fs::read_to_string(fixture_path("refs/minimal_refs.sql")).expect("refs fixture exists");
    let conn = Connection::open(path).expect("open refs sqlite");
    conn.execute_batch(&sql).expect("create refs fixture");
}

fn run_fixture(name: &str, input_relative: &str, extra_args: &[&str]) -> (PathBuf, String, String) {
    let dir = unique_temp_dir(name);
    let refs = dir.join("refs.sqlite");
    let out = dir.join("profile.sqlite");
    create_refs_from_fixture(&refs);

    let mut args = vec![
        fixture_path(input_relative).display().to_string(),
        "--refs".to_string(),
        refs.display().to_string(),
        "--out".to_string(),
        out.display().to_string(),
    ];
    args.extend(extra_args.iter().map(|arg| arg.to_string()));

    let output = run_profile(&args);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(out.is_file());
    (out, stdout(&output), stderr(&output))
}

fn run_fixture_with_config(
    name: &str,
    input_relative: &str,
    config_relative: &str,
    extra_args: &[&str],
) -> (PathBuf, String, String) {
    let config_path = fixture_path(config_relative);
    let mut args = vec!["--config", config_path.to_str().unwrap()];
    args.extend_from_slice(extra_args);
    run_fixture(name, input_relative, &args)
}

fn sqlite_i64(db: &Path, sql: &str) -> i64 {
    let conn = Connection::open(db).expect("open profile sqlite");
    conn.query_row(sql, [], |row| row.get(0))
        .expect("query scalar")
}

fn assert_schema_contract(db: &Path) {
    assert_eq!(
        sqlite_i64(db, "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view'"),
        0
    );
    assert_eq!(
        sqlite_i64(
            db,
            "\
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'table'
              AND (
                name IN (
                  'prof_path_sample',
                  'prof_shape_sample',
                  'prof_run',
                  'prof_manifest',
                  'prof_algorithm',
                  'prof_warning'
                )
                OR name LIKE 'prof_array_%'
              )
            "
        ),
        0
    );

    let conn = Connection::open(db).expect("open profile sqlite");
    let tables = conn
        .prepare(
            "\
            SELECT name
            FROM sqlite_master
            WHERE type = 'table'
              AND name LIKE 'prof_%'
            ORDER BY name
            ",
        )
        .expect("prepare approved tables")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query approved tables")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect approved tables");

    assert_eq!(
        tables,
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

fn assert_no_priority_bound_violations(db: &Path, limit: i64) {
    assert_eq!(
        sqlite_i64(
            db,
            &format!(
                "\
                SELECT COUNT(*)
                FROM (
                  SELECT sample_scope, sample_key, COUNT(*) AS c
                  FROM prof_object_sample
                  WHERE sample_kind = 'priority_sample'
                  GROUP BY sample_scope, sample_key
                  HAVING c > {limit}
                )
                "
            )
        ),
        0
    );
}

#[test]
fn basic_json_fixture_populates_summary_shapes_and_field_profiles() {
    let (db, stdout, stderr) = run_fixture("fixture-basic-json", "json/basic.json", &[]);

    assert!(stdout.contains("documents: 1"));
    assert_eq!(stderr, "");
    assert_eq!(
        sqlite_i64(&db, "SELECT COUNT(*) FROM prof_source_summary"),
        1
    );
    assert!(sqlite_i64(&db, "SELECT COUNT(*) FROM prof_shape") > 0);
    assert!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_shape_field
            WHERE field_name IN ('status', 'amount')
            "
        ) >= 2
    );
    assert_schema_contract(&db);
}

#[test]
fn basic_jsonl_fixture_counts_documents() {
    let (db, stdout, _) = run_fixture("fixture-basic-jsonl", "jsonl/basic.jsonl", &["--jsonl"]);

    assert!(stdout.contains("documents: 3"));
    assert_eq!(
        sqlite_i64(&db, "SELECT total_document_count FROM prof_source_summary"),
        3
    );
    assert_schema_contract(&db);
}

#[test]
fn heterogeneous_shape_fixture_splits_shapes_and_field_profiles() {
    let (db, _, _) = run_fixture(
        "fixture-heterogeneous-shape",
        "json/heterogeneous_shape.json",
        &[],
    );

    assert!(
        sqlite_i64(
            &db,
            "SELECT COUNT(*) FROM prof_shape WHERE canonical_path = '$[]'"
        ) >= 3
    );
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(DISTINCT observed_type)
            FROM prof_shape_field
            WHERE field_name = 'amount'
            "
        ),
        2
    );
    assert_schema_contract(&db);
}

#[test]
fn heterogeneous_object_array_uses_shape_rows_not_array_tables() {
    let (db, _, _) = run_fixture(
        "fixture-heterogeneous-object-array",
        "json/heterogeneous_object_array.json",
        &[],
    );

    assert!(
        sqlite_i64(
            &db,
            "SELECT COUNT(*) FROM prof_shape WHERE site_path LIKE '%items%'"
        ) >= 3
    );
    assert_schema_contract(&db);
}

#[test]
fn empty_first_seen_then_non_empty_fixture_captures_both_samples() {
    let (db, _, _) = run_fixture(
        "fixture-empty-then-non-empty",
        "jsonl/empty_then_non_empty.jsonl",
        &["--jsonl"],
    );

    let conn = Connection::open(&db).expect("open profile sqlite");
    let first_seen: String = conn
        .query_row(
            "\
            SELECT sample_json
            FROM prof_object_sample
            WHERE canonical_path = '$.payload'
              AND sample_scope = 'canonical_path'
              AND sample_kind = 'first_seen'
            ",
            [],
            |row| row.get(0),
        )
        .expect("query first_seen payload sample");
    let first_non_empty: String = conn
        .query_row(
            "\
            SELECT sample_json
            FROM prof_object_sample
            WHERE canonical_path = '$.payload'
              AND sample_scope = 'canonical_path'
              AND sample_kind = 'first_non_empty'
            ",
            [],
            |row| row.get(0),
        )
        .expect("query first_non_empty payload sample");

    assert_eq!(first_seen, "{}");
    assert_ne!(first_non_empty, "{}");
    assert!(first_non_empty.contains("\"id\""));
}

#[test]
fn empty_string_fixture_counts_empty_string_and_samples_it_as_non_empty() {
    let (db, _, _) = run_fixture(
        "fixture-empty-string",
        "jsonl/empty_string.jsonl",
        &["--jsonl"],
    );

    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT s.empty_string_count
            FROM prof_field_summary AS s
            JOIN prof_shape_field AS f ON f.field_profile_id = s.field_profile_id
            WHERE f.field_name = 'code'
              AND f.observed_type = 'string'
            "
        ),
        1
    );
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_value_sample AS sample
            JOIN prof_shape_field AS f ON f.field_profile_id = sample.field_profile_id
            WHERE f.field_name = 'code'
              AND f.observed_type = 'string'
              AND sample.sample_kind = 'first_non_empty'
              AND sample.value_json = '\"\"'
            "
        ),
        1
    );
}

#[test]
fn exact_distribution_fixture_writes_complete_exact_counts() {
    let (db, _, _) = run_fixture(
        "fixture-exact-distribution",
        "jsonl/exact_distribution.jsonl",
        &["--jsonl"],
    );

    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_summary AS s
            JOIN prof_shape_field AS f ON f.field_profile_id = s.field_profile_id
            WHERE f.field_name = 'status'
              AND s.distinct_count_method = 'exact'
            "
        ),
        1
    );
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_value AS v
            JOIN prof_shape_field AS f ON f.field_profile_id = v.field_profile_id
            WHERE f.field_name = 'status'
              AND v.value_source = 'exact_full'
              AND v.count_method = 'exact'
              AND v.is_complete_distribution = 1
            "
        ),
        3
    );
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT v.count
            FROM prof_field_value AS v
            JOIN prof_shape_field AS f ON f.field_profile_id = v.field_profile_id
            WHERE f.field_name = 'status'
              AND v.value_text = 'A'
            "
        ),
        2
    );
}

#[test]
fn approximate_fallback_fixture_writes_hll_and_bounded_heavy_hitters() {
    let (db, _, _) = run_fixture_with_config(
        "fixture-approx-fallback",
        "jsonl/approx_fallback.jsonl",
        "config/approx_fallback.yaml",
        &["--jsonl"],
    );

    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_summary AS s
            JOIN prof_shape_field AS f ON f.field_profile_id = s.field_profile_id
            WHERE f.field_name = 'id'
              AND s.distinct_count_method = 'approximate'
              AND s.distinct_algorithm = 'hyperloglog'
            "
        ),
        1
    );
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_value AS v
            JOIN prof_shape_field AS f ON f.field_profile_id = v.field_profile_id
            WHERE f.field_name = 'id'
              AND v.is_complete_distribution != 0
            "
        ),
        0
    );
    assert!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM prof_field_value AS v
            JOIN prof_shape_field AS f ON f.field_profile_id = v.field_profile_id
            WHERE f.field_name = 'id'
              AND v.value_source = 'heavy_hitter'
            "
        ) <= 8
    );
}

#[test]
fn sample_oom_guard_fixture_bounds_priority_samples_and_preserves_first_seen() {
    let (db, _, _) = run_fixture_with_config(
        "fixture-sample-guard",
        "jsonl/sample_guard.jsonl",
        "config/sample_guard.yaml",
        &["--jsonl"],
    );

    assert_no_priority_bound_violations(&db, 1);
    assert_eq!(
        sqlite_i64(
            &db,
            "\
            SELECT COUNT(*)
            FROM (
              SELECT sample_scope, sample_key
              FROM prof_object_sample
              GROUP BY sample_scope, sample_key

              EXCEPT

              SELECT sample_scope, sample_key
              FROM prof_object_sample
              WHERE sample_kind = 'first_seen'
            )
            "
        ),
        0
    );
}

#[test]
fn rc2_diagnose_script_enforces_performance_safe_sample_contract() {
    let (db, _, _) = run_fixture(
        "fixture-rc2-diagnose-defaults",
        "jsonl/sample_guard.jsonl",
        &["--jsonl"],
    );

    let output = Command::new("bash")
        .arg(script_path("diagnose_profile_sqlite.sh"))
        .arg("--fail-on-risk")
        .arg("--hh-context-limit")
        .arg("0")
        .arg("--value-sample-limit")
        .arg("4")
        .arg(&db)
        .output()
        .expect("run profile sqlite diagnosis");

    assert!(
        output.status.success(),
        "diagnose_profile_sqlite.sh reported rc.2 regression risk\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn regression_scripts_are_checked_in_and_syntax_valid() {
    for script in [
        "assert_profile_sqlite.sh",
        "diagnose_profile_sqlite.sh",
        "make_fixture_refs.sh",
        "make_large_jsonl_fixture.py",
        "regression_profile_json_refs_v0_1_rc2_patch.sh",
    ] {
        assert!(script_path(script).is_file(), "missing script {script}");
    }

    for script in [
        "assert_profile_sqlite.sh",
        "diagnose_profile_sqlite.sh",
        "make_fixture_refs.sh",
        "regression_profile_json_refs_v0_1_rc2_patch.sh",
    ] {
        let output = Command::new("bash")
            .arg("-n")
            .arg(script_path(script))
            .output()
            .expect("run bash -n");
        assert!(
            output.status.success(),
            "script {script} has invalid shell syntax: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let output = Command::new("python3")
        .arg("-m")
        .arg("py_compile")
        .arg(script_path("make_large_jsonl_fixture.py"))
        .output()
        .expect("run python py_compile");
    assert!(
        output.status.success(),
        "large fixture generator has invalid python syntax: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rc2_regression_script_exits_zero_after_successful_cleanup() {
    let dir = unique_temp_dir("rc2-regression-script-cleanup");
    let dump_bin = dir.join("dump-json-refs-stub");
    let profile_bin = dir.join("profile-json-refs-stub");

    fs::write(
        &dump_bin,
        r#"#!/usr/bin/env bash
set -euo pipefail
mkdir -p refs
: > refs/schemas.sqlite
"#,
    )
    .expect("write dump stub");
    make_executable(&dump_bin);

    fs::write(
        &profile_bin,
        r#"#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--help" ]]; then
  echo "Usage: profile-json-refs <INPUT>"
  exit 0
fi

if [[ "${1:-}" == "-" ]]; then
  echo "stdin is not supported" >&2
  exit 2
fi

out=""
quiet=0
perf=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --strict)
      echo "--strict is not supported" >&2
      exit 2
      ;;
    --out)
      out="${2:?--out requires value}"
      shift 2
      ;;
    --quiet)
      quiet=1
      shift
      ;;
    --perf-log)
      perf=1
      shift
      ;;
    *)
      shift
      ;;
  esac
done

[[ -n "$out" ]] || { echo "--out is required" >&2; exit 2; }

sqlite3 "$out" <<'SQL'
CREATE TABLE prof_source_summary(total_document_count INTEGER NOT NULL);
CREATE TABLE prof_shape(shape_id TEXT NOT NULL);
CREATE TABLE prof_shape_field(field_profile_id TEXT NOT NULL);
CREATE TABLE prof_field_summary(
  field_profile_id TEXT NOT NULL,
  distinct_count_method TEXT NOT NULL,
  empty_string_count INTEGER NOT NULL
);
CREATE TABLE prof_field_value(field_profile_id TEXT NOT NULL, value_source TEXT NOT NULL);
CREATE TABLE prof_field_value_sample(field_profile_id TEXT NOT NULL, sample_kind TEXT NOT NULL);
CREATE TABLE prof_object_sample(sample_kind TEXT NOT NULL, sample_scope TEXT NOT NULL, sample_key TEXT NOT NULL);
INSERT INTO prof_source_summary(total_document_count) VALUES (83);
INSERT INTO prof_shape(shape_id) VALUES ('shape:1');
INSERT INTO prof_shape_field(field_profile_id) VALUES ('field:1');
INSERT INTO prof_field_summary(field_profile_id, distinct_count_method, empty_string_count)
VALUES ('field:1', 'approximate', 1);
INSERT INTO prof_field_value(field_profile_id, value_source) VALUES ('field:1', 'exact_full');
SQL

if [[ "$quiet" -eq 0 ]]; then
  {
    echo "profile-json-refs: wrote $out"
    echo "documents: 83"
    echo "objects: 83"
    echo "shapes: 1"
    echo "field_profiles: 1"
    echo "elapsed: 0.000s"
  }
fi

if [[ "$perf" -eq 1 ]]; then
  echo "[perf] total 0ms" >&2
fi
"#,
    )
    .expect("write profile stub");
    make_executable(&profile_bin);

    let output = Command::new("bash")
        .arg(script_path(
            "regression_profile_json_refs_v0_1_rc2_patch.sh",
        ))
        .env("PROFILE_JSON_REFS_BIN", &profile_bin)
        .env("DUMP_JSON_REFS_BIN", &dump_bin)
        .env("TMPDIR", &dir)
        .output()
        .expect("run rc2 regression script");

    assert!(
        output.status.success(),
        "rc2 regression script should exit zero after successful checks\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
