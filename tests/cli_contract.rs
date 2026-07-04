use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use profile_json_refs::cli::CliArgs;
use profile_json_refs::config::{InputFormat, ProfileConfig};
use profile_json_refs::error::ProfileError;

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

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, b"").expect("write fixture file");
}

fn parse_config(args: &[&str]) -> ProfileConfig {
    let args = CliArgs::try_parse_from(args).expect("CLI args should parse");
    ProfileConfig::from_cli(args).expect("config should load")
}

#[test]
fn defaults_use_contract_paths_and_auto_format() {
    let config = parse_config(&["profile-json-refs", "data.json"]);

    assert_eq!(config.input_file, PathBuf::from("data.json"));
    assert_eq!(config.refs_sqlite, PathBuf::from("refs/schemas.sqlite"));
    assert_eq!(config.out_sqlite, PathBuf::from("profile.sqlite"));
    assert_eq!(config.input_format, InputFormat::Auto);
    assert!(!config.quiet);
    assert!(!config.perf_log);
}

#[test]
fn jsonl_flag_forces_jsonl_format() {
    let config = parse_config(&["profile-json-refs", "data.jsonl", "--jsonl"]);

    assert_eq!(config.input_format, InputFormat::Jsonl);
}

#[test]
fn strict_flag_is_not_part_of_the_cli_contract() {
    let err = CliArgs::try_parse_from(["profile-json-refs", "data.json", "--strict"])
        .expect_err("--strict should be rejected by clap");

    assert!(err.to_string().contains("unexpected argument"));
}

#[test]
fn missing_input_file_argument_rejects_stdin_pipeline_shape() {
    let err = CliArgs::try_parse_from(["profile-json-refs", "--jsonl"])
        .expect_err("input file is required");

    assert!(err.to_string().contains("required"));
}

#[test]
fn dash_input_is_rejected_by_validation() {
    let config = parse_config(&["profile-json-refs", "-"]);

    let err = config
        .validate()
        .expect_err("dash input should be rejected");
    assert!(matches!(err, ProfileError::StdinUnsupported));
}

#[test]
fn unknown_yaml_key_fails_early() {
    let dir = unique_temp_dir("unknown-yaml");
    let config_path = dir.join("profile.yaml");
    fs::write(&config_path, "unknown: true\n").expect("write config");

    let args = CliArgs::try_parse_from([
        "profile-json-refs",
        "data.json",
        "--config",
        config_path.to_str().expect("utf8 path"),
    ])
    .expect("CLI args should parse");

    let err = ProfileConfig::from_cli(args).expect_err("unknown YAML key should fail");
    assert!(matches!(err, ProfileError::ConfigParse { .. }));
}

#[test]
fn yaml_overrides_defaults_when_cli_option_is_absent() {
    let dir = unique_temp_dir("yaml-overrides-defaults");
    let refs_path = dir.join("refs.sqlite");
    let out_path = dir.join("profile-from-yaml.sqlite");
    let config_path = dir.join("profile.yaml");
    fs::write(
        &config_path,
        format!(
            r#"
input:
  format: json
refs:
  sqlite: {}
output:
  sqlite: {}
stdout:
  quiet: true
perf:
  log: true
value_profile:
  hll_precision: 12
  heavy_hitter_limit: 64
"#,
            refs_path.display(),
            out_path.display()
        ),
    )
    .expect("write config");

    let config = parse_config(&[
        "profile-json-refs",
        "data.json",
        "--config",
        config_path.to_str().expect("utf8 path"),
    ]);

    assert_eq!(config.refs_sqlite, refs_path);
    assert_eq!(config.out_sqlite, out_path);
    assert_eq!(config.input_format, InputFormat::Json);
    assert!(config.quiet);
    assert!(config.perf_log);
    assert_eq!(config.value_profile.hll_precision, 12);
    assert_eq!(config.value_profile.heavy_hitter_limit, 64);
}

#[test]
fn cli_options_override_yaml_config() {
    let dir = unique_temp_dir("cli-overrides-yaml");
    let yaml_refs = dir.join("yaml-refs.sqlite");
    let cli_refs = dir.join("cli-refs.sqlite");
    let yaml_out = dir.join("yaml-profile.sqlite");
    let cli_out = dir.join("cli-profile.sqlite");
    let config_path = dir.join("profile.yaml");
    fs::write(
        &config_path,
        format!(
            r#"
input:
  format: json
refs:
  sqlite: {}
output:
  sqlite: {}
sampling:
  object:
    type_set:
      priority_sample_limit: 2
  value:
    priority_sample_limit_per_field_profile: 3
value_profile:
  value_text_limit_bytes: 256
  hll_precision: 10
  heavy_hitter_limit: 32
"#,
            yaml_refs.display(),
            yaml_out.display()
        ),
    )
    .expect("write config");

    let config = parse_config(&[
        "profile-json-refs",
        "data.jsonl",
        "--config",
        config_path.to_str().expect("utf8 path"),
        "--refs",
        cli_refs.to_str().expect("utf8 path"),
        "--out",
        cli_out.to_str().expect("utf8 path"),
        "--jsonl",
        "--shape-sample-limit",
        "6",
        "--value-sample-limit",
        "9",
        "--heavy-hitter-limit",
        "128",
        "--hll-precision",
        "15",
        "--value-text-limit",
        "2048",
        "--quiet",
        "--perf-log",
    ]);

    assert_eq!(config.refs_sqlite, cli_refs);
    assert_eq!(config.out_sqlite, cli_out);
    assert_eq!(config.input_format, InputFormat::Jsonl);
    assert_eq!(config.sampling.type_set_priority_limit, 6);
    assert_eq!(config.sampling.value_priority_limit_per_field_profile, 9);
    assert_eq!(config.value_profile.heavy_hitter_limit, 128);
    assert_eq!(config.value_profile.hll_precision, 15);
    assert_eq!(config.value_profile.value_text_limit_bytes, 2048);
    assert!(config.quiet);
    assert!(config.perf_log);
}

#[test]
fn invalid_hll_precision_fails_validation() {
    let dir = unique_temp_dir("invalid-hll");
    let input = dir.join("data.json");
    let refs = dir.join("refs.sqlite");
    touch(&input);
    touch(&refs);

    let config = parse_config(&[
        "profile-json-refs",
        input.to_str().expect("utf8 path"),
        "--refs",
        refs.to_str().expect("utf8 path"),
        "--hll-precision",
        "3",
    ]);

    let err = config
        .validate()
        .expect_err("invalid hll precision should fail");
    assert!(
        matches!(err, ProfileError::InvalidConfig(message) if message.contains("hll_precision"))
    );
}

#[test]
fn heavy_hitter_limit_zero_fails_validation() {
    let dir = unique_temp_dir("zero-heavy-hitter");
    let input = dir.join("data.json");
    let refs = dir.join("refs.sqlite");
    touch(&input);
    touch(&refs);

    let config = parse_config(&[
        "profile-json-refs",
        input.to_str().expect("utf8 path"),
        "--refs",
        refs.to_str().expect("utf8 path"),
        "--heavy-hitter-limit",
        "0",
    ]);

    let err = config
        .validate()
        .expect_err("zero heavy hitter limit should fail");
    assert!(
        matches!(err, ProfileError::InvalidConfig(message) if message.contains("heavy_hitter_limit"))
    );
}

#[test]
fn shape_sample_limit_zero_fails_validation() {
    let dir = unique_temp_dir("zero-shape-sample");
    let input = dir.join("data.json");
    let refs = dir.join("refs.sqlite");
    touch(&input);
    touch(&refs);

    let config = parse_config(&[
        "profile-json-refs",
        input.to_str().expect("utf8 path"),
        "--refs",
        refs.to_str().expect("utf8 path"),
        "--shape-sample-limit",
        "0",
    ]);

    let err = config
        .validate()
        .expect_err("zero shape sample limit should fail");
    assert!(matches!(err, ProfileError::InvalidConfig(message) if message.contains("type_set")));
}
