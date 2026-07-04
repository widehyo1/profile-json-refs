mod common;

use std::fs;

use common::{create_refs_db, run_profile, stderr, stdout, unique_temp_dir};
use rusqlite::Connection;

fn max_group_count(conn: &Connection, sql: &str) -> u64 {
    conn.query_row(sql, [], |row| row.get::<_, Option<u64>>(0))
        .expect("query max group count")
        .unwrap_or(0)
}

#[test]
fn large_finite_jsonl_run_keeps_persisted_samples_bounded() {
    let dir = unique_temp_dir("perf-smoke");
    let input = dir.join("input.jsonl");
    let refs = dir.join("refs.sqlite");
    let out = dir.join("profile.sqlite");
    let config = dir.join("profile.yaml");

    let mut jsonl = String::new();
    for index in 0..250 {
        jsonl.push_str(&format!(
            r#"{{"id":{index},"group":"g{}","payload":"value-{index}"}}"#,
            index % 5
        ));
        jsonl.push('\n');
    }
    fs::write(&input, jsonl).expect("write jsonl smoke input");
    create_refs_db(&refs, false);
    fs::write(
        &config,
        r#"
sampling:
  object:
    canonical_path:
      priority_sample_limit: 1
    site_path:
      priority_sample_limit: 1
    field_set:
      priority_sample_limit: 1
    type_set:
      priority_sample_limit: 1
  value:
    priority_sample_limit_per_field_profile: 2
    heavy_hitter_context_sample_limit: 0
flush:
  chunk_object_sample_rows: 2
  chunk_value_sample_rows: 2
  chunk_shape_rows: 2
  chunk_field_rows: 2
value_profile:
  exact_distinct_threshold: 4
  heavy_hitter_limit: 3
  hll_precision: 8
"#,
    )
    .expect("write smoke config");

    let output = run_profile(&[
        input.display().to_string(),
        "--jsonl".to_string(),
        "--config".to_string(),
        config.display().to_string(),
        "--refs".to_string(),
        refs.display().to_string(),
        "--out".to_string(),
        out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("documents: 250"));
    assert!(stderr(&output).contains("[perf] scan.read_parse_walk="));
    assert!(out.metadata().expect("profile sqlite metadata").len() < 1_000_000);

    let conn = Connection::open(&out).expect("open output profile");
    let array_table_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name LIKE 'prof_array_%'",
            [],
            |row| row.get(0),
        )
        .expect("query forbidden array tables");
    assert_eq!(array_table_count, 0);

    assert_eq!(
        max_group_count(
            &conn,
            "\
            SELECT MAX(cnt)
            FROM (
                SELECT COUNT(*) AS cnt
                FROM prof_object_sample
                WHERE sample_kind = 'priority_sample'
                GROUP BY sample_scope, sample_key
            )
            "
        ),
        1
    );
    assert!(
        max_group_count(
            &conn,
            "\
            SELECT MAX(cnt)
            FROM (
                SELECT COUNT(*) AS cnt
                FROM prof_field_value_sample
                WHERE sample_kind = 'priority_sample'
                GROUP BY field_profile_id
            )
            "
        ) <= 2
    );
    assert_eq!(
        max_group_count(
            &conn,
            "\
            SELECT MAX(cnt)
            FROM (
                SELECT COUNT(*) AS cnt
                FROM prof_field_value_sample
                WHERE sample_kind = 'heavy_hitter_context'
                GROUP BY field_profile_id, value_hash
            )
            "
        ),
        0
    );
    assert!(
        max_group_count(
            &conn,
            "\
            SELECT MAX(cnt)
            FROM (
                SELECT COUNT(*) AS cnt
                FROM prof_field_value
                WHERE value_source = 'heavy_hitter'
                GROUP BY field_profile_id
            )
            "
        ) <= 3
    );

    let id_profile: (String, String, Option<f64>, u64) = conn
        .query_row(
            "\
            SELECT
                s.distinct_count_method,
                COALESCE(s.distinct_algorithm, ''),
                s.distinct_error_rate,
                s.stored_value_count
            FROM prof_field_summary AS s
            JOIN prof_shape_field AS f ON f.field_profile_id = s.field_profile_id
            WHERE f.field_name = 'id'
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("query id field profile summary");

    assert_eq!(id_profile.0, "approximate");
    assert_eq!(id_profile.1, "hyperloglog");
    assert!(id_profile.2.is_some());
    assert!(id_profile.3 <= 3);
}
