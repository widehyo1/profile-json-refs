use std::collections::HashSet;

use profile_json_refs::config::SamplingConfig;
use profile_json_refs::refs::resolver::ResolvedObjectContext;
use profile_json_refs::scan::path::SourcePath;
use profile_json_refs::shape::accumulator::ShapeAccumulator;
use profile_json_refs::shape::sample::{ObjectSampleKind, SampleScope, object_sample_key};
use serde_json::{Map, Value};

fn object(raw: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(raw).unwrap() {
        Value::Object(object) => object,
        _ => panic!("test fixture must be a JSON object"),
    }
}

fn source_path(raw: &str) -> SourcePath {
    let mut path = SourcePath::root();
    for part in raw
        .strip_prefix("$.items")
        .expect("test path should be under $.items")
        .split('[')
        .skip(1)
    {
        let index = part.trim_end_matches(']').parse::<usize>().unwrap();
        path.push_field("items");
        path.push_index(index);
    }
    path
}

fn context() -> ResolvedObjectContext {
    ResolvedObjectContext {
        canonical_path: "$.items[]".to_string(),
        site_path: Some("$.items[]".to_string()),
        schema_path: "#/items".to_string(),
        resolved: true,
    }
}

#[test]
fn every_object_sample_scope_has_first_seen() {
    let mut accumulator = ShapeAccumulator::default();
    let path = source_path("$.items[0]");
    let object = object(r#"{"id":1,"type":"A"}"#);

    accumulator
        .observe_object(0, &path, &context(), &object, &SamplingConfig::default())
        .unwrap();

    let rows = accumulator.drain_object_sample_rows();
    let first_seen: HashSet<(SampleScope, String)> = rows
        .iter()
        .filter(|row| row.sample_kind == ObjectSampleKind::FirstSeen)
        .map(|row| (row.sample_scope, row.sample_key.clone()))
        .collect();

    let type_set_row = accumulator
        .shape_rows()
        .into_iter()
        .find(|row| row.canonical_path == "$.items[]")
        .unwrap();
    for scope in [
        SampleScope::CanonicalPath,
        SampleScope::SitePath,
        SampleScope::FieldSet,
        SampleScope::TypeSet,
    ] {
        let key = object_sample_key(
            scope,
            "$.items[]",
            Some("$.items[]"),
            Some(&type_set_row.field_set_hash),
            Some(&type_set_row.type_set_hash),
        );
        assert!(first_seen.contains(&(scope, key)));
    }
}

#[test]
fn empty_first_seen_does_not_block_later_first_non_empty() {
    let mut accumulator = ShapeAccumulator::default();
    let config = SamplingConfig::default();
    let empty_path = source_path("$.items[0]");
    let non_empty_path = source_path("$.items[1]");
    let empty = object("{}");
    let non_empty = object(r#"{"id":1}"#);

    accumulator
        .observe_object(0, &empty_path, &context(), &empty, &config)
        .unwrap();
    accumulator
        .observe_object(1, &non_empty_path, &context(), &non_empty, &config)
        .unwrap();

    let rows = accumulator.drain_object_sample_rows();
    let canonical_key = "$.items[]";
    let first_seen = rows
        .iter()
        .find(|row| {
            row.sample_scope == SampleScope::CanonicalPath
                && row.sample_key == canonical_key
                && row.sample_kind == ObjectSampleKind::FirstSeen
        })
        .unwrap();
    let first_non_empty = rows
        .iter()
        .find(|row| {
            row.sample_scope == SampleScope::CanonicalPath
                && row.sample_key == canonical_key
                && row.sample_kind == ObjectSampleKind::FirstNonEmpty
        })
        .unwrap();

    assert_eq!(first_seen.document_index, 0);
    assert_eq!(first_seen.sample_json, "{}");
    assert_eq!(first_non_empty.document_index, 1);
    assert_eq!(first_non_empty.sample_json, r#"{"id":1}"#);
}

#[test]
fn priority_object_samples_are_bounded_per_scope_and_key_on_flush() {
    let mut accumulator = ShapeAccumulator::default();
    let config = SamplingConfig {
        canonical_priority_limit: 1,
        site_priority_limit: 1,
        field_set_priority_limit: 1,
        type_set_priority_limit: 2,
        ..SamplingConfig::default()
    };

    for index in 0..20 {
        let mut path = SourcePath::root();
        path.push_field("items");
        path.push_index(index);
        let object = object(&format!(r#"{{"id":{index},"type":"A"}}"#));
        accumulator
            .observe_object(index as u64, &path, &context(), &object, &config)
            .unwrap();
    }

    let rows = accumulator.drain_object_sample_rows();
    let priority_rows: Vec<_> = rows
        .iter()
        .filter(|row| row.sample_kind == ObjectSampleKind::PrioritySample)
        .collect();

    let counts_for_scope = |scope| {
        priority_rows
            .iter()
            .filter(|row| row.sample_scope == scope)
            .count()
    };

    assert_eq!(counts_for_scope(SampleScope::CanonicalPath), 1);
    assert_eq!(counts_for_scope(SampleScope::SitePath), 1);
    assert_eq!(counts_for_scope(SampleScope::FieldSet), 1);
    assert_eq!(counts_for_scope(SampleScope::TypeSet), 2);
}
