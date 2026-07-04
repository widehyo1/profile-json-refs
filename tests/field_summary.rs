use std::path::PathBuf;

use profile_json_refs::config::{
    FlushConfig, InputFormat, ProfileConfig, SamplingConfig, ValueProfileConfig,
};
use profile_json_refs::field::accumulator::FieldAccumulator;
use profile_json_refs::field::id::field_profile_id;
use profile_json_refs::field::summary::{DistinctCountMethod, FieldSummary, update_summary};
use profile_json_refs::util::json_type::JsonType;
use serde_json::{Map, Value, json};

fn profile_config() -> ProfileConfig {
    ProfileConfig {
        input_file: PathBuf::from("input.json"),
        refs_sqlite: PathBuf::from("refs/schemas.sqlite"),
        out_sqlite: PathBuf::from("profile.sqlite"),
        input_format: InputFormat::Json,
        quiet: false,
        perf_log: false,
        sampling: SamplingConfig::default(),
        value_profile: ValueProfileConfig::default(),
        flush: FlushConfig::default(),
    }
}

fn object(raw: Value) -> Map<String, Value> {
    match raw {
        Value::Object(object) => object,
        _ => panic!("test fixture must be an object"),
    }
}

#[test]
fn field_summary_counts_null_empty_object_empty_array_and_empty_string() {
    let mut summary = FieldSummary {
        field_profile_id: "field-a".to_string(),
        ..FieldSummary::default()
    };

    update_summary(&mut summary, &Value::Null);
    update_summary(&mut summary, &json!({}));
    update_summary(&mut summary, &json!([]));
    update_summary(&mut summary, &json!(""));

    assert_eq!(summary.profiled_count, 4);
    assert_eq!(summary.null_count, 1);
    assert_eq!(summary.non_null_count, 3);
    assert_eq!(summary.empty_object_count, 1);
    assert_eq!(summary.empty_array_count, 1);
    assert_eq!(summary.empty_string_count, 1);
    assert_eq!(
        summary.distinct_count_method,
        DistinctCountMethod::Unavailable
    );
}

#[test]
fn field_accumulator_tracks_shape_specific_rows_and_summaries() {
    let mut accumulator = FieldAccumulator::default();
    let config = profile_config();
    let object = object(json!({
        "nullable": null,
        "empty_object": {},
        "empty_array": [],
        "empty_string": ""
    }));
    let parent = Value::Object(object.clone());

    accumulator.observe_object_fields(7, "$", "shape-a", &object, &parent, &config);

    let rows = accumulator.shape_field_rows();
    assert_eq!(rows.len(), 4);

    let nullable_id = field_profile_id("shape-a", "nullable", JsonType::Null);
    let nullable = rows
        .iter()
        .find(|row| row.field_profile_id == nullable_id)
        .unwrap();
    assert_eq!(nullable.observed_count, 1);
    assert_eq!(nullable.null_count, 1);

    let summaries = accumulator.field_summaries();
    let empty_string = summaries
        .iter()
        .find(|summary| {
            summary.field_profile_id
                == field_profile_id("shape-a", "empty_string", JsonType::String)
        })
        .unwrap();
    assert_eq!(empty_string.profiled_count, 1);
    assert_eq!(empty_string.non_null_count, 1);
    assert_eq!(empty_string.empty_string_count, 1);
}

#[test]
fn field_profile_id_separates_shape_and_observed_type_contexts() {
    let shape_a_string = field_profile_id("shape-a", "status", JsonType::String);
    let shape_b_string = field_profile_id("shape-b", "status", JsonType::String);
    let shape_a_null = field_profile_id("shape-a", "status", JsonType::Null);

    assert_ne!(shape_a_string, shape_b_string);
    assert_ne!(shape_a_string, shape_a_null);
}
