#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JsonType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Object,
    Array,
    Unknown,
}

impl JsonType {
    pub fn from_value(value: &serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => JsonType::Null,
            serde_json::Value::Bool(_) => JsonType::Boolean,
            serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => {
                JsonType::Integer
            }
            serde_json::Value::Number(_) => JsonType::Number,
            serde_json::Value::String(_) => JsonType::String,
            serde_json::Value::Array(_) => JsonType::Array,
            serde_json::Value::Object(_) => JsonType::Object,
        }
    }

    pub fn as_sql_str(self) -> &'static str {
        match self {
            JsonType::Null => "null",
            JsonType::Boolean => "boolean",
            JsonType::Integer => "integer",
            JsonType::Number => "number",
            JsonType::String => "string",
            JsonType::Object => "object",
            JsonType::Array => "array",
            JsonType::Unknown => "unknown",
        }
    }
}
