use std::path::PathBuf;

use profile_json_refs::config::{
    FlushConfig, InputFormat, ProfileConfig, SamplingConfig, ValueProfileConfig,
};
use profile_json_refs::field::accumulator::FieldValueAccumulator;
use profile_json_refs::value::sample::ValueSampleKind;
use serde_json::json;

fn profile_config() -> ProfileConfig {
    ProfileConfig {
        input_file: PathBuf::from("input.json"),
        refs_sqlite: PathBuf::from("refs/schemas.sqlite"),
        out_sqlite: PathBuf::from("profile.sqlite"),
        input_format: InputFormat::Json,
        quiet: false,
        perf_log: false,
        perf_log_file: None,
        perf_log_dbstat: false,
        sampling: SamplingConfig {
            value_priority_limit_per_field_profile: 2,
            heavy_hitter_context_sample_limit: 0,
            ..SamplingConfig::default()
        },
        value_profile: ValueProfileConfig {
            exact_distinct_threshold: 2,
            heavy_hitter_limit: 2,
            ..ValueProfileConfig::default()
        },
        flush: FlushConfig::default(),
    }
}

#[test]
fn value_samples_capture_first_seen_and_first_non_empty() {
    let config = profile_config();
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    accumulator.observe(0, "$.field", &json!(null), &json!({"field": null}), &config);
    accumulator.observe(1, "$.field", &json!(""), &json!({"field": ""}), &config);

    let rows = accumulator.value_sample_rows();
    let first_seen = rows
        .iter()
        .find(|row| row.sample_kind == ValueSampleKind::FirstSeen)
        .unwrap();
    let first_non_empty = rows
        .iter()
        .find(|row| row.sample_kind == ValueSampleKind::FirstNonEmpty)
        .unwrap();

    assert_eq!(first_seen.document_index, 0);
    assert_eq!(first_seen.value_json.as_deref(), Some("null"));
    assert_eq!(first_non_empty.document_index, 1);
    assert_eq!(first_non_empty.value_json.as_deref(), Some(r#""""#));
}

#[test]
fn value_priority_samples_are_bounded() {
    let config = profile_config();
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    for index in 0..20 {
        accumulator.observe(
            index,
            "$.field",
            &json!(format!("value-{index}")),
            &json!({"field": format!("value-{index}")}),
            &config,
        );
    }

    let rows = accumulator.value_sample_rows();
    let priority_count = rows
        .iter()
        .filter(|row| row.sample_kind == ValueSampleKind::PrioritySample)
        .count();
    assert_eq!(
        priority_count,
        config.sampling.value_priority_limit_per_field_profile
    );
}

#[test]
fn heavy_hitter_context_samples_are_not_emitted_in_rc2() {
    let mut config = profile_config();
    config.sampling.heavy_hitter_context_sample_limit = 4;
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    for index in 0..10 {
        accumulator.observe(
            index,
            "$.field",
            &json!("hot"),
            &json!({"field": "hot"}),
            &config,
        );
    }

    let rows = accumulator.finish(&config).value_samples;
    let context_count = rows
        .iter()
        .filter(|row| row.sample_kind == ValueSampleKind::HeavyHitterContext)
        .count();
    assert_eq!(context_count, 0);
}
