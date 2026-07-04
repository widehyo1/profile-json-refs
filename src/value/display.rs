#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayValue {
    pub text: Option<String>,
    pub truncated: bool,
}

pub fn value_text(value: &serde_json::Value, limit: usize) -> DisplayValue {
    let raw = match value {
        serde_json::Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string()),
    };
    let (text, truncated) = crate::util::truncate::truncate_utf8(&raw, limit);

    DisplayValue {
        text: Some(text),
        truncated,
    }
}
