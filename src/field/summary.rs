#[derive(Debug, Default, Clone, PartialEq)]
pub struct FieldSummary {
    pub field_profile_id: String,
    pub profiled_count: u64,
    pub null_count: u64,
    pub non_null_count: u64,
    pub empty_object_count: u64,
    pub empty_array_count: u64,
    pub empty_string_count: u64,
    pub distinct_count: Option<u64>,
    pub distinct_count_method: DistinctCountMethod,
    pub distinct_algorithm: Option<DistinctAlgorithm>,
    pub distinct_error_rate: Option<f64>,
    pub stored_value_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistinctCountMethod {
    Exact,
    Approximate,
    #[default]
    Unavailable,
}

impl DistinctCountMethod {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            DistinctCountMethod::Exact => "exact",
            DistinctCountMethod::Approximate => "approximate",
            DistinctCountMethod::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinctAlgorithm {
    HyperLogLog,
}

impl DistinctAlgorithm {
    pub fn as_sql_str(self) -> &'static str {
        match self {
            DistinctAlgorithm::HyperLogLog => "hyperloglog",
        }
    }
}

pub fn update_summary(summary: &mut FieldSummary, value: &serde_json::Value) {
    summary.profiled_count += 1;

    match value {
        serde_json::Value::Null => {
            summary.null_count += 1;
        }
        serde_json::Value::Object(object) => {
            summary.non_null_count += 1;
            if object.is_empty() {
                summary.empty_object_count += 1;
            }
        }
        serde_json::Value::Array(array) => {
            summary.non_null_count += 1;
            if array.is_empty() {
                summary.empty_array_count += 1;
            }
        }
        serde_json::Value::String(text) => {
            summary.non_null_count += 1;
            if text.is_empty() {
                summary.empty_string_count += 1;
            }
        }
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            summary.non_null_count += 1;
        }
    }
}
