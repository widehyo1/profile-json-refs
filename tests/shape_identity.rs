use profile_json_refs::shape::id::compute_shape_facts;
use serde_json::{Map, Value};

fn object(raw: &str) -> Map<String, Value> {
    match serde_json::from_str::<Value>(raw).unwrap() {
        Value::Object(object) => object,
        _ => panic!("test fixture must be a JSON object"),
    }
}

#[test]
fn same_shape_input_produces_stable_shape_id() {
    let object = object(r#"{"type":"A","amount":100,"id":1}"#);

    let first = compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &object);
    let second = compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &object);

    assert_eq!(first.shape_id, second.shape_id);
    assert_eq!(first.field_set_hash, second.field_set_hash);
    assert_eq!(first.type_set_hash, second.type_set_hash);
    assert_eq!(first.field_set_json, r#"["amount","id","type"]"#);

    let type_pairs: Vec<(String, String)> = serde_json::from_str(&first.type_set_json).unwrap();
    assert_eq!(
        type_pairs,
        vec![
            ("amount".to_string(), "integer".to_string()),
            ("id".to_string(), "integer".to_string()),
            ("type".to_string(), "string".to_string()),
        ]
    );
}

#[test]
fn same_field_set_with_different_type_set_produces_different_shape_id() {
    let integer_amount = object(r#"{"id":1,"type":"A","amount":100}"#);
    let string_amount = object(r#"{"id":2,"type":"A","amount":"100"}"#);

    let integer_shape =
        compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &integer_amount);
    let string_shape =
        compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &string_amount);

    assert_eq!(integer_shape.field_set_hash, string_shape.field_set_hash);
    assert_ne!(integer_shape.type_set_hash, string_shape.type_set_hash);
    assert_ne!(integer_shape.shape_id, string_shape.shape_id);
}

#[test]
fn different_field_set_produces_different_shape_id() {
    let amount_object = object(r#"{"id":1,"type":"A","amount":100}"#);
    let error_object = object(r#"{"id":2,"type":"B","error":"invalid"}"#);

    let amount_shape =
        compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &amount_object);
    let error_shape = compute_shape_facts("$.items[]", Some("$.items[]"), "#/items", &error_object);

    assert_ne!(amount_shape.field_set_hash, error_shape.field_set_hash);
    assert_ne!(amount_shape.shape_id, error_shape.shape_id);
}
