mod common;

use std::fs;

use common::{basic_fixture, create_refs_db, run_profile, stderr, stdout, unique_temp_dir};

const REQUIRED_BUCKETS: &[&str] = &[
    "total",
    "refs.open",
    "refs.load_contract",
    "sqlite.create_schema",
    "scan.read_parse_walk",
    "scan.flush_shapes",
    "scan.flush_fields",
    "scan.flush_values",
    "scan.flush_samples",
    "sqlite.indexes",
    "stdout.summary",
    "main.total_wall",
];

fn assert_perf_contains(stderr: &str, needle: &str) {
    assert!(
        stderr.contains(needle),
        "missing perf diagnostic {needle:?} in stderr:\n{stderr}"
    );
}

#[test]
fn perf_log_emits_required_buckets_to_stderr() {
    let fixture = basic_fixture("perf-log", r#"{"id":1,"name":"Ada"}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stderr = stderr(&output);
    for bucket in REQUIRED_BUCKETS {
        assert!(
            stderr.contains(&format!("[perf] {bucket}=")),
            "missing perf bucket {bucket} in stderr:\n{stderr}"
        );
    }
}

#[test]
fn perf_log_does_not_affect_stdout_summary() {
    let fixture = basic_fixture("perf-log-stdout", r#"{"id":1}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("profile-json-refs: wrote"));
    assert!(!stdout.contains("[perf]"));
    assert!(stderr(&output).contains("[perf] total="));
}

#[test]
fn perf_log_file_writes_events_outside_stderr() {
    let fixture = basic_fixture("perf-log-file", r#"{"id":1}"#, false);
    let perf_log = fixture.out.with_file_name("perf.log");

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
        "--perf-log-file".to_string(),
        perf_log.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!stderr(&output).contains("[perf]"));
    let perf = std::fs::read_to_string(&perf_log).expect("read perf log");
    assert!(perf.contains("[perf]"));
    assert!(perf.contains("phase=scan.progress") || perf.contains("scan.read_parse_walk"));
}

#[test]
fn perf_log_dbstat_is_opt_in() {
    let fixture = basic_fixture("perf-log-dbstat", r#"{"id":1}"#, false);

    let without = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);
    assert!(without.status.success(), "stderr: {}", stderr(&without));
    assert!(!stderr(&without).contains("phase=sqlite.dbstat"));

    let with = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
        "--perf-log-dbstat".to_string(),
    ]);
    assert!(with.status.success(), "stderr: {}", stderr(&with));
    assert!(stderr(&with).contains("phase=sqlite.dbstat"));
    assert!(stderr(&with).contains("rank=1"));
    assert!(stderr(&with).contains("table=") || stderr(&with).contains("unavailable=1"));
}

#[test]
fn perf_log_emits_scan_and_sqlite_detail_events() {
    let fixture = basic_fixture(
        "perf-log-density",
        r#"{"id":1,"name":"Ada","tags":["engineer"]}
{"id":2,"name":"Grace","tags":["compiler","navy"]}"#,
        true,
    );

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--jsonl".to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stderr = stderr(&output);

    assert_perf_contains(&stderr, "phase=scan.progress");
    assert_perf_contains(&stderr, "phase=scan.chunk");
    assert_perf_contains(&stderr, "parse_ms=");
    assert_perf_contains(&stderr, "walk_ms=");
    assert_perf_contains(&stderr, "docs_per_sec=");
    assert_perf_contains(&stderr, "scalars_per_sec=");
    assert_perf_contains(&stderr, "phase=scan.accumulators");
    assert_perf_contains(&stderr, "pending_shapes=");
    assert_perf_contains(&stderr, "pending_shape_fields=");
    assert_perf_contains(&stderr, "pending_value_samples=");
    assert_perf_contains(&stderr, "phase=scan.hot_counters");
    assert_perf_contains(&stderr, "objects_visited=");
    assert_perf_contains(&stderr, "arrays_visited=");
    assert_perf_contains(&stderr, "scalars_visited=");
    assert_perf_contains(&stderr, "field_edges_visited=");
    assert_perf_contains(&stderr, "scalar_nulls=");
    assert_perf_contains(&stderr, "scalar_booleans=");
    assert_perf_contains(&stderr, "scalar_integers=");
    assert_perf_contains(&stderr, "scalar_numbers=");
    assert_perf_contains(&stderr, "scalar_strings=");
    assert_perf_contains(&stderr, "shape_observations=");
    assert_perf_contains(&stderr, "field_profile_observations=");
    assert_perf_contains(&stderr, "value_observations=");
    assert_perf_contains(&stderr, "flush_checks=");
    assert!(
        !stderr.contains("value_sample_candidates="),
        "scan.hot_counters should not report value sample candidate deltas from global pending counts:\n{stderr}"
    );
    assert!(
        !stderr.contains("object_sample_candidates="),
        "scan.hot_counters should not report object sample candidate deltas from sample maps:\n{stderr}"
    );
    assert_perf_contains(&stderr, "phase=scan.sampled_walk");
    assert_perf_contains(&stderr, "sample_interval=");
    assert_perf_contains(&stderr, "sampled_documents=");
    assert_perf_contains(&stderr, "documents_since_last=");
    assert_perf_contains(&stderr, "sample_ratio=");
    assert_perf_contains(&stderr, "path_ms=");
    assert_perf_contains(&stderr, "value_hash_ms=");
    assert_perf_contains(&stderr, "value_canonicalize_ms=");
    assert_perf_contains(&stderr, "field_update_ms=");
    assert_perf_contains(&stderr, "sample_update_ms=");

    assert_perf_contains(&stderr, "phase=flush.trigger");
    assert_perf_contains(&stderr, "phase=flush.trigger index=0 reason=final_samples");
    assert_perf_contains(&stderr, "pending_value_samples=");
    assert_perf_contains(&stderr, "phase=flush.chunk");
    assert_perf_contains(&stderr, "field_summaries=");
    assert_perf_contains(&stderr, "field_values=");

    assert_perf_contains(&stderr, "phase=sqlite.flush.shapes");
    assert_perf_contains(&stderr, "phase=sqlite.flush.shape_fields");
    assert_perf_contains(&stderr, "phase=sqlite.flush.object_samples");
    assert_perf_contains(&stderr, "phase=sqlite.flush.field_summaries");
    assert_perf_contains(&stderr, "phase=sqlite.flush.field_values");
    assert_perf_contains(&stderr, "phase=sqlite.flush.value_samples");
    assert_perf_contains(&stderr, "phase=sqlite.flush.commit");
    assert_perf_contains(&stderr, "elapsed_ms=");
    assert_perf_contains(&stderr, "rows=");
    assert_perf_contains(&stderr, "rows_deleted=");

    assert_perf_contains(&stderr, "phase=scan.chunk index=0 reason=final");
    assert!(
        !stderr.contains("phase=scan.chunk index=0 reason=progress"),
        "scan completion should not emit duplicate progress and final chunks for the same window:\n{stderr}"
    );
}

#[test]
fn value_sample_limit_flush_reports_o1_pending_sample_counter() {
    let dir = unique_temp_dir("perf-value-sample-limit");
    let input = dir.join("input.jsonl");
    let refs = dir.join("refs.sqlite");
    let out = dir.join("profile.sqlite");
    let config = dir.join("profile.yaml");

    fs::write(
        &input,
        r#"{"id":1,"name":"Ada"}
{"id":2,"name":"Grace"}
"#,
    )
    .expect("write input");
    create_refs_db(&refs, false);
    fs::write(
        &config,
        r#"
flush:
  chunk_object_sample_rows: 1000
  chunk_value_sample_rows: 2
  chunk_shape_rows: 1000
  chunk_field_rows: 1000
"#,
    )
    .expect("write config");

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
    assert!(stdout(&output).contains("documents: 2"));
    let stderr = stderr(&output);
    assert_perf_contains(&stderr, "phase=flush.trigger");
    assert_perf_contains(&stderr, "reason=value_sample_limit");
    assert_perf_contains(&stderr, "pending_value_samples=");
}

#[test]
fn field_value_hot_path_avoids_heavy_hitter_key_materialization() {
    let source =
        std::fs::read_to_string("src/field/accumulator.rs").expect("read field accumulator");
    let observe_inner = source
        .split("fn observe_inner(")
        .nth(1)
        .and_then(|tail| tail.split("pub fn value_sample_rows(").next())
        .expect("extract FieldValueAccumulator::observe_inner");

    assert!(
        !observe_inner.contains("heavy_hitters.keys()"),
        "field value hot path must not materialize heavy hitter keys for per-observation cleanup"
    );
    assert!(
        !observe_inner.contains("format!(\"{key:?}\")"),
        "field value hot path must not allocate debug-formatted ValueKey hash input"
    );
}

#[test]
fn perf_hot_counters_do_not_add_aggregate_work_to_visit_object() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let visit_object = source
        .split("fn visit_object(")
        .nth(1)
        .and_then(|tail| tail.split("fn visit_array(").next())
        .expect("extract visit_object implementation");

    assert!(
        !visit_object.contains("pending_value_sample_count()"),
        "visit_object must not call pending_value_sample_count() for diagnostics"
    );
    assert!(
        !visit_object.contains("pending_object_sample_count()"),
        "visit_object must not call pending_object_sample_count() for diagnostics"
    );
    assert!(
        !visit_object.contains("Instant::now()"),
        "visit_object must not add per-object timing"
    );
    assert!(
        visit_object.contains("flush_after_object_if_needed()"),
        "visit_object should use the object hot-path flush check"
    );

    let flush_after_object = source
        .split("fn flush_after_object_if_needed(")
        .nth(1)
        .and_then(|tail| tail.split("fn flush_pending_samples(").next())
        .expect("extract object hot-path flush implementation");
    assert!(
        !flush_after_object.contains("pending_value_sample_count()"),
        "object hot-path flush must not scan field value accumulators"
    );
    assert!(
        !flush_after_object.contains("pending_object_sample_count()"),
        "object hot-path flush must not scan object sample maps"
    );

    let flush_if_needed = source
        .split("fn flush_if_needed(")
        .nth(1)
        .and_then(|tail| tail.split("fn flush_after_object_if_needed(").next())
        .expect("extract document flush implementation");
    assert!(
        !flush_if_needed.contains("pending_value_sample_count_slow()"),
        "document flush checks must use the maintained pending value sample counter"
    );
    assert!(
        flush_if_needed.contains("pending_value_sample_rows"),
        "document flush checks should compare the O(1) pending value sample counter"
    );

    let visit_array = source
        .split("fn visit_array(")
        .nth(1)
        .and_then(|tail| tail.split("fn visit_scalar(").next())
        .expect("extract visit_array implementation");
    let visit_scalar = source
        .split("fn visit_scalar(")
        .nth(1)
        .and_then(|tail| tail.split("fn field_source_path(").next())
        .expect("extract visit_scalar implementation");

    assert!(
        !visit_array.contains("Instant::now()"),
        "visit_array must not add per-array timing"
    );
    assert!(
        !visit_scalar.contains("Instant::now()"),
        "visit_scalar must not add per-scalar timing"
    );
}

#[test]
fn perf_flush_work_runs_outside_scan_walk_timing() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let flush_pending_samples = source
        .split("fn flush_pending_samples(")
        .nth(1)
        .and_then(|tail| tail.split("fn flush_chunk(").next())
        .expect("extract flush_pending_samples implementation");

    let pause = flush_pending_samples
        .find("pause_scan_walk_timing()")
        .expect("flush_pending_samples pauses walk timing");
    let diagnostics = flush_pending_samples
        .find("emit_flush_diagnostics")
        .expect("flush_pending_samples emits diagnostics");
    let flush = flush_pending_samples
        .find("flush_chunk_while_paused")
        .expect("flush_pending_samples flushes while paused");
    let resume = flush_pending_samples
        .find("resume_scan_walk_timing")
        .expect("flush_pending_samples resumes walk timing");
    assert!(
        pause < diagnostics && diagnostics < flush && flush < resume,
        "flush diagnostics and SQLite writes must stay outside scan walk timing"
    );

    let flush_chunk = source
        .split("fn flush_chunk(")
        .nth(1)
        .and_then(|tail| tail.split("fn flush_chunk_while_paused(").next())
        .expect("extract flush_chunk implementation");
    let pause = flush_chunk
        .find("pause_scan_walk_timing()")
        .expect("flush_chunk pauses walk timing");
    let flush = flush_chunk
        .find("flush_chunk_while_paused")
        .expect("flush_chunk flushes while paused");
    let resume = flush_chunk
        .find("resume_scan_walk_timing")
        .expect("flush_chunk resumes walk timing");
    assert!(
        pause < flush && flush < resume,
        "explicit chunk flush must stay outside scan walk timing"
    );
}

#[test]
fn perf_log_emits_final_sqlite_summary_and_size_events() {
    let fixture = basic_fixture("perf-log-final-density", r#"{"id":1,"name":"Ada"}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--perf-log".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stderr = stderr(&output);

    assert_perf_contains(&stderr, "phase=sqlite.prune.object_priority");
    assert_perf_contains(&stderr, "phase=sqlite.prune.object_priority elapsed_ms=");
    assert_perf_contains(&stderr, "rows_deleted=");
    assert_perf_contains(&stderr, "phase=sqlite.prune.value_priority");
    assert_perf_contains(&stderr, "phase=sqlite.prune.value_priority elapsed_ms=");
    assert_perf_contains(&stderr, "rows_deleted=");
    assert_perf_contains(&stderr, "phase=sqlite.summary.counts");
    assert_perf_contains(&stderr, "phase=sqlite.summary.write");
    assert_perf_contains(&stderr, "phase=sqlite.size");
    assert_perf_contains(&stderr, "profile_sqlite_bytes=");
    assert_perf_contains(&stderr, "phase=sqlite.close");
    assert_perf_contains(&stderr, "closed=1");
}
