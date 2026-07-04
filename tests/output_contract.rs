mod common;

use common::{basic_fixture, run_profile, stderr, stdout, unique_temp_dir};

#[test]
fn default_stdout_contains_output_path_summary_counts_and_elapsed_only() {
    let fixture = basic_fixture("stdout-default", r#"{"id":1,"name":"Ada"}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains(&format!(
        "profile-json-refs: wrote {}",
        fixture.out.display()
    )));
    assert!(stdout.contains("documents: 1"));
    assert!(stdout.contains("objects: 1"));
    assert!(stdout.contains("arrays: 0"));
    assert!(stdout.contains("scalars: 2"));
    assert!(stdout.contains("canonical_paths: 1"));
    assert!(stdout.contains("site_paths: 1"));
    assert!(stdout.contains("shapes: 1"));
    assert!(stdout.contains("field_profiles: 2"));
    assert!(stdout.contains("stored_values: 2"));
    assert!(stdout.contains("elapsed: "));
    assert!(!stdout.contains("WARNING"));
    assert!(!stdout.contains("[perf]"));
    assert_eq!(stderr(&output), "");
    assert!(fixture.out.is_file());
}

#[test]
fn quiet_suppresses_normal_success_stdout() {
    let fixture = basic_fixture("stdout-quiet", r#"{"id":1}"#, false);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
        "--quiet".to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "");
    assert!(fixture.out.is_file());
}

#[test]
fn warnings_are_stderr_only_and_do_not_block_output() {
    let fixture = basic_fixture("stderr-warning", r#"{"id":1}"#, true);

    let output = run_profile(&[
        fixture.input.display().to_string(),
        "--refs".to_string(),
        fixture.refs.display().to_string(),
        "--out".to_string(),
        fixture.out.display().to_string(),
    ]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let stdout = stdout(&output);
    let stderr = stderr(&output);
    assert!(stdout.contains("profile-json-refs: wrote"));
    assert!(!stdout.contains("W_REFS_PRESENCE_SHAPES_TRUNCATED"));
    assert!(stderr.contains("WARNING W_REFS_PRESENCE_SHAPES_TRUNCATED"));
    assert!(fixture.out.is_file());
}

#[test]
fn errors_are_stderr_and_non_zero() {
    let dir = unique_temp_dir("stderr-error");
    let refs = dir.join("refs.sqlite");
    common::create_refs_db(&refs, false);
    let missing_input = dir.join("missing.json");
    let out = dir.join("profile.sqlite");

    let output = run_profile(&[
        missing_input.display().to_string(),
        "--refs".to_string(),
        refs.display().to_string(),
        "--out".to_string(),
        out.display().to_string(),
    ]);

    assert!(!output.status.success());
    assert_eq!(stdout(&output), "");
    assert!(stderr(&output).contains("ERROR input path is not a file"));
    assert!(!out.exists());
}
