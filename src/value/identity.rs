use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKey {
    Null,
    Bool(bool),
    Integer(String),
    Number(String),
    String(String),
    ObjectHash(String),
    ArrayHash(String),
}

impl ValueKey {
    pub fn json_type(&self) -> crate::util::json_type::JsonType {
        match self {
            ValueKey::Null => crate::util::json_type::JsonType::Null,
            ValueKey::Bool(_) => crate::util::json_type::JsonType::Boolean,
            ValueKey::Integer(_) => crate::util::json_type::JsonType::Integer,
            ValueKey::Number(_) => crate::util::json_type::JsonType::Number,
            ValueKey::String(_) => crate::util::json_type::JsonType::String,
            ValueKey::ObjectHash(_) => crate::util::json_type::JsonType::Object,
            ValueKey::ArrayHash(_) => crate::util::json_type::JsonType::Array,
        }
    }
}

pub fn value_key(value: &Value) -> ValueKey {
    match value {
        Value::Null => ValueKey::Null,
        Value::Bool(value) => ValueKey::Bool(*value),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            ValueKey::Integer(number.to_string())
        }
        Value::Number(number) => ValueKey::Number(number.to_string()),
        Value::String(value) => ValueKey::String(value.clone()),
        Value::Object(_) => {
            let canonical = canonical_json(value);
            ValueKey::ObjectHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
        Value::Array(_) => {
            let canonical = canonical_json(value);
            ValueKey::ArrayHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
    }
}

pub fn value_hash(value: &Value) -> String {
    value_hash_from_key(&value_key(value))
}

pub fn value_hash_from_key(key: &ValueKey) -> String {
    crate::util::hash::stable_hex(format!("{key:?}").as_bytes())
}

pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).expect("JSON scalar serializes")
        }
        Value::Array(array) => {
            let values: Vec<_> = array.iter().map(canonical_json).collect();
            format!("[{}]", values.join(","))
        }
        Value::Object(object) => {
            let mut fields: Vec<_> = object.iter().collect();
            fields.sort_by(|(left, _), (right, _)| left.cmp(right));
            let values: Vec<_> = fields
                .into_iter()
                .map(|(field, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(field).expect("object key serializes"),
                        canonical_json(value)
                    )
                })
                .collect();
            format!("{{{}}}", values.join(","))
        }
    }
}
