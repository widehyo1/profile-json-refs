use std::time::{Duration, Instant};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ValueKey {
    Null,
    Bool(bool),
    Integer(String),
    Number(String),
    String(String),
    ObjectHash(String),
    ArrayHash(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValueHashTiming {
    pub value_hash_elapsed: Duration,
    pub value_canonicalize_elapsed: Duration,
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

    pub fn stable_hash64(&self) -> u64 {
        let mut hasher = crate::util::hash::StableHasher::new();
        match self {
            ValueKey::Null => hasher.update(b"Null"),
            ValueKey::Bool(value) => {
                hasher.update(b"Bool(");
                hasher.update(if *value { b"true" } else { b"false" });
                hasher.update(b")");
            }
            ValueKey::Integer(value) => hash_debug_string_variant(&mut hasher, b"Integer", value),
            ValueKey::Number(value) => hash_debug_string_variant(&mut hasher, b"Number", value),
            ValueKey::String(value) => hash_debug_string_variant(&mut hasher, b"String", value),
            ValueKey::ObjectHash(value) => {
                hash_debug_string_variant(&mut hasher, b"ObjectHash", value);
            }
            ValueKey::ArrayHash(value) => {
                hash_debug_string_variant(&mut hasher, b"ArrayHash", value);
            }
        }
        hasher.finish()
    }
}

fn hash_debug_string_variant(
    hasher: &mut crate::util::hash::StableHasher,
    variant: &[u8],
    value: &str,
) {
    hasher.update(variant);
    hasher.update(b"(\"");
    for character in value.chars() {
        if character == '\'' {
            hasher.update(b"'");
        } else {
            for escaped in character.escape_debug() {
                let mut buffer = [0; 4];
                hasher.update(escaped.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    hasher.update(b"\")");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedValue {
    pub key: ValueKey,
    pub stable_hash64: u64,
    pub value_type: crate::util::json_type::JsonType,
}

impl ObservedValue {
    pub fn from_value(value: &Value) -> Self {
        Self::from_key(value_key(value))
    }

    pub fn from_value_with_canonical_timing(
        value: &Value,
        value_canonicalize_elapsed: &mut Duration,
    ) -> Self {
        Self::from_key(value_key_with_canonical_timing(
            value,
            value_canonicalize_elapsed,
        ))
    }

    fn from_key(key: ValueKey) -> Self {
        let stable_hash64 = key.stable_hash64();
        let value_type = key.json_type();
        Self {
            key,
            stable_hash64,
            value_type,
        }
    }
}

pub fn value_key(value: &Value) -> ValueKey {
    value_key_inner(value, None)
}

pub fn value_key_with_canonical_timing(
    value: &Value,
    value_canonicalize_elapsed: &mut Duration,
) -> ValueKey {
    value_key_inner(value, Some(value_canonicalize_elapsed))
}

fn value_key_inner(value: &Value, value_canonicalize_elapsed: Option<&mut Duration>) -> ValueKey {
    match value {
        Value::Null => ValueKey::Null,
        Value::Bool(value) => ValueKey::Bool(*value),
        Value::Number(number) if number.is_i64() || number.is_u64() => {
            ValueKey::Integer(number.to_string())
        }
        Value::Number(number) => ValueKey::Number(number.to_string()),
        Value::String(value) => ValueKey::String(value.clone()),
        Value::Object(_) => {
            let canonical = canonical_json_timed(value, value_canonicalize_elapsed);
            ValueKey::ObjectHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
        Value::Array(_) => {
            let canonical = canonical_json_timed(value, value_canonicalize_elapsed);
            ValueKey::ArrayHash(crate::util::hash::stable_hex(canonical.as_bytes()))
        }
    }
}

pub fn value_hash(value: &Value) -> String {
    value_hash_from_key(&value_key(value))
}

pub fn value_hash_with_timing(value: &Value, timing: &mut ValueHashTiming) -> String {
    let started = Instant::now();
    let key = value_key_with_canonical_timing(value, &mut timing.value_canonicalize_elapsed);
    let hash = value_hash_from_key(&key);
    timing.value_hash_elapsed += started.elapsed();
    hash
}

pub fn value_hash_from_key(key: &ValueKey) -> String {
    crate::util::hash::stable_hex(format!("{key:?}").as_bytes())
}

fn canonical_json_timed(value: &Value, elapsed: Option<&mut Duration>) -> String {
    if let Some(elapsed) = elapsed {
        let started = Instant::now();
        let canonical = canonical_json(value);
        *elapsed += started.elapsed();
        canonical
    } else {
        canonical_json(value)
    }
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
