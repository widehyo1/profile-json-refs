use profile_json_refs::value::display::value_text;
use profile_json_refs::value::identity::{ValueKey, value_hash, value_key};
use serde_json::{Value, json};

#[test]
fn value_key_preserves_scalar_type_boundaries() {
    assert_eq!(value_key(&Value::Null), ValueKey::Null);
    assert_eq!(value_key(&json!(true)), ValueKey::Bool(true));
    assert_eq!(value_key(&json!(1)), ValueKey::Integer("1".to_string()));
    assert_eq!(value_key(&json!("1")), ValueKey::String("1".to_string()));
    assert_ne!(value_hash(&json!(1)), value_hash(&json!("1")));
}

#[test]
fn value_hash_for_objects_is_canonical_by_object_key_order() {
    let first: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
    let second: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();

    assert_eq!(value_hash(&first), value_hash(&second));
}

#[test]
fn value_text_uses_plain_string_text_and_utf8_truncation() {
    let display = value_text(&json!("abcdef"), 4);
    assert_eq!(display.text.as_deref(), Some("abcd"));
    assert!(display.truncated);

    let display = value_text(&json!("가나다"), 4);
    assert_eq!(display.text.as_deref(), Some("가"));
    assert!(display.truncated);
}
