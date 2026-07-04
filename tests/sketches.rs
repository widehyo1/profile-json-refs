use std::path::PathBuf;

use profile_json_refs::config::{
    FlushConfig, InputFormat, ProfileConfig, SamplingConfig, ValueProfileConfig,
};
use profile_json_refs::field::accumulator::FieldValueAccumulator;
use profile_json_refs::field::summary::{DistinctAlgorithm, DistinctCountMethod};
use profile_json_refs::sketch::hll::HyperLogLog;
use profile_json_refs::sketch::space_saving::SpaceSaving;
use profile_json_refs::util::hash::stable_u64;
use profile_json_refs::value::exact_counter::{CountMethod, ValueSource};
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
        sampling: SamplingConfig::default(),
        value_profile: ValueProfileConfig {
            exact_distinct_threshold: 2,
            exact_value_bytes_per_field_profile: 1024,
            hll_precision: 10,
            heavy_hitter_limit: 2,
            ..ValueProfileConfig::default()
        },
        flush: FlushConfig::default(),
    }
}

#[test]
fn hyperloglog_estimate_and_error_rate_are_populated() {
    let mut hll = HyperLogLog::new(10);

    for value in 0..100 {
        hll.insert_hash(stable_u64(value.to_string().as_bytes()));
    }

    let estimate = hll.estimate();
    assert!((70..=140).contains(&estimate), "estimate was {estimate}");
    assert!(hll.relative_error() > 0.0);
}

#[test]
fn space_saving_keeps_bounded_heavy_hitter_candidates() {
    let mut sketch = SpaceSaving::new(2);

    for value in ["hot", "hot", "hot", "warm", "hot", "cold", "hot"] {
        sketch.observe(value.to_string());
    }

    let top = sketch.top();
    assert!(top.len() <= 2);
    assert_eq!(top[0].0, "hot");
    assert!(top[0].1.count >= 5);
}

#[test]
fn priority_sampler_reports_admission_before_materialization() {
    let mut sampler = profile_json_refs::sketch::priority::PrioritySampler::new(2);

    assert!(sampler.should_accept(20));
    sampler.push(20, "twenty");
    assert!(sampler.should_accept(10));
    sampler.push(10, "ten");
    assert!(sampler.should_accept(5));
    assert!(!sampler.should_accept(30));
}

#[test]
fn approximate_profile_uses_hll_and_bounded_heavy_hitter_rows() {
    let config = profile_config();
    let parent = json!({"field": "value"});
    let mut accumulator = FieldValueAccumulator::new("field-a".to_string(), &config);

    for index in 0..20 {
        accumulator.observe(
            index,
            "$.field",
            &json!(format!("value-{index}")),
            &parent,
            &config,
        );
    }

    let output = accumulator.finish(&config);
    assert_eq!(
        output.summary.distinct_count_method,
        DistinctCountMethod::Approximate
    );
    assert_eq!(
        output.summary.distinct_algorithm,
        Some(DistinctAlgorithm::HyperLogLog)
    );
    assert!(output.summary.distinct_error_rate.is_some());
    assert!(output.summary.distinct_count.unwrap() > 0);

    let heavy_hitters: Vec<_> = output
        .field_values
        .iter()
        .filter(|row| row.value_source == ValueSource::HeavyHitter)
        .collect();
    assert!(heavy_hitters.len() <= config.value_profile.heavy_hitter_limit);
    assert!(
        heavy_hitters
            .iter()
            .all(|row| row.count_method == CountMethod::Approximate)
    );
    assert!(
        heavy_hitters
            .iter()
            .all(|row| !row.is_complete_distribution)
    );
}
