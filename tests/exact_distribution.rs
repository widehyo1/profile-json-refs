use profile_json_refs::value::exact_counter::{CountMethod, ExactCounter, ValueSource};
use profile_json_refs::value::identity::value_key;
use serde_json::json;

#[test]
fn categorical_field_below_threshold_materializes_exact_full_rows() {
    let mut counter = ExactCounter::new(8, 1024);

    counter.observe(&json!("red"));
    counter.observe(&json!("blue"));
    counter.observe(&json!("red"));

    assert!(counter.is_enabled());
    assert_eq!(counter.distinct_count(), Some(2));

    let rows = counter.exact_full_rows("field-a", 64);
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.value_source == ValueSource::ExactFull)
    );
    assert!(
        rows.iter()
            .all(|row| row.count_method == CountMethod::Exact)
    );
    assert!(rows.iter().all(|row| row.is_complete_distribution));

    let red = rows
        .iter()
        .find(|row| row.value_text.as_deref() == Some("red"))
        .unwrap();
    assert_eq!(red.count, Some(2));
}

#[test]
fn crossing_exact_distinct_threshold_disables_exact_full_rows() {
    let mut counter = ExactCounter::new(2, 1024);

    counter.observe(&json!("a"));
    counter.observe(&json!("b"));
    counter.observe(&json!("c"));

    assert!(!counter.is_enabled());
    assert_eq!(counter.distinct_count(), None);
    assert!(counter.exact_full_rows("field-a", 64).is_empty());
}

#[test]
fn crossing_per_field_byte_budget_disables_exact_full_rows() {
    let mut counter = ExactCounter::new(10, 8);

    counter.observe(&json!("abcdef"));
    counter.observe(&json!("ghijkl"));

    assert!(!counter.is_enabled());
    assert_eq!(counter.distinct_count(), None);
    assert!(counter.exact_full_rows("field-a", 64).is_empty());
}

#[test]
fn keyed_observation_matches_value_observation_and_preserves_caps() {
    let values = [
        json!("red"),
        json!("blue"),
        json!("red"),
        json!({"nested": true}),
    ];
    let mut direct = ExactCounter::new(8, 1024);
    let mut keyed = ExactCounter::new(8, 1024);

    for value in &values {
        direct.observe(value);
        let key = value_key(value);
        keyed.observe_keyed(&key, value);
    }

    assert_eq!(
        direct.exact_full_rows("field-a", 64),
        keyed.exact_full_rows("field-a", 64)
    );

    let mut capped = ExactCounter::new(2, 1024);
    for value in [json!("a"), json!("b"), json!("c")] {
        let key = value_key(&value);
        capped.observe_keyed(&key, &value);
    }
    assert!(!capped.is_enabled());
    assert_eq!(capped.distinct_count(), None);
}
