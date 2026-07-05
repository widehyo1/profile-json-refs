use std::path::PathBuf;

use profile_json_refs::config::{
    FlushConfig, InputFormat, ProfileConfig, SamplingConfig, ValueProfileConfig,
};
use profile_json_refs::field::accumulator::FieldValueAccumulator;
use profile_json_refs::value::sample::{ValueSampleAccumulator, ValueSampleKind};
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

    let null_parent = json!({"field": null});
    accumulator.observe(
        0,
        "$.field",
        &json!(null),
        null_parent
            .as_object()
            .expect("parent fixture is an object"),
        &config,
    );
    let empty_parent = json!({"field": ""});
    accumulator.observe(
        1,
        "$.field",
        &json!(""),
        empty_parent
            .as_object()
            .expect("parent fixture is an object"),
        &config,
    );

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
        let parent = json!({"field": format!("value-{index}")});
        accumulator.observe(
            index,
            "$.field",
            &json!(format!("value-{index}")),
            parent.as_object().expect("parent fixture is an object"),
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
fn field_value_observation_stats_match_pending_sample_rows_and_drain() {
    let config = profile_config();
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);
    let parent = json!({"field": "alpha"});
    let value = json!("alpha");

    let stats = accumulator.observe(
        0,
        "$.field",
        &value,
        parent.as_object().expect("parent fixture is an object"),
        &config,
    );

    assert_eq!(
        stats.pending_value_sample_delta.added,
        accumulator.pending_value_sample_count()
    );
    assert_eq!(stats.pending_value_sample_delta.removed, 0);
    assert_eq!(
        accumulator.drain_value_sample_rows().len(),
        stats.pending_value_sample_delta.added
    );
    assert_eq!(accumulator.pending_value_sample_count(), 0);
}

#[test]
fn heavy_hitter_context_samples_are_not_emitted_in_rc2() {
    let mut config = profile_config();
    config.sampling.heavy_hitter_context_sample_limit = 4;
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    for index in 0..10 {
        let parent = json!({"field": "hot"});
        accumulator.observe(
            index,
            "$.field",
            &json!("hot"),
            parent.as_object().expect("parent fixture is an object"),
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

#[test]
fn value_sample_observation_reports_pending_row_delta() {
    let config = profile_config();
    let mut accumulator = ValueSampleAccumulator::new(1, 0);
    let parent = json!({"field": "alpha"});
    let parent = parent.as_object().expect("parent fixture is an object");
    let value = json!("alpha");

    let delta = accumulator.observe(0, "$.field", "field-a", &value, parent, &config);

    assert_eq!(delta.added, accumulator.pending_row_count());
    assert_eq!(delta.removed, 0);
    assert!(delta.net() > 0);
}

#[test]
fn value_sample_priority_replacement_does_not_grow_pending_count() {
    let config = profile_config();
    let mut accumulator = ValueSampleAccumulator::new(1, 0);
    let mut previous_priority_sample_id = None;

    for index in 0..10_000 {
        let value = json!(format!("value-{index}"));
        let parent = json!({"field": value.clone()});
        let before_count = accumulator.pending_row_count();
        let delta = accumulator.observe(
            index,
            &format!("$.field{index}"),
            "field-a",
            &value,
            parent.as_object().expect("parent fixture is an object"),
            &config,
        );
        let after_count = accumulator.pending_row_count();
        let priority_sample_id = accumulator
            .rows()
            .into_iter()
            .find(|row| row.sample_kind == ValueSampleKind::PrioritySample)
            .map(|row| row.value_sample_id);

        if index > 0 && priority_sample_id != previous_priority_sample_id {
            assert_eq!(after_count, before_count);
            assert_eq!(delta.net(), 0);
            return;
        }

        previous_priority_sample_id = priority_sample_id;
    }

    panic!("expected deterministic priority replacement within fixture search");
}
