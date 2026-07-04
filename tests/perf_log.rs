mod common;

use common::{basic_fixture, run_profile, stderr, stdout};

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
    "sqlite.prune_samples",
    "sqlite.indexes",
    "stdout.summary",
];

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
