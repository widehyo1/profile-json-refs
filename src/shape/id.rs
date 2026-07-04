use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeKey {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_set_hash: String,
    pub type_set_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeFacts {
    pub shape_id: String,
    pub field_set_hash: String,
    pub type_set_hash: String,
    pub field_set_json: String,
    pub type_set_json: String,
}

pub fn compute_shape_facts(
    canonical_path: &str,
    site_path: Option<&str>,
    schema_path: &str,
    object: &Map<String, Value>,
) -> ShapeFacts {
    let mut field_names: Vec<&str> = object.keys().map(String::as_str).collect();
    field_names.sort_unstable();
    let field_set_json = serde_json::to_string(&field_names).expect("field names serialize");
    let field_set_hash = crate::util::hash::stable_hex(field_set_json.as_bytes());

    let mut type_pairs: Vec<(String, String)> = object
        .iter()
        .map(|(field, value)| {
            (
                field.clone(),
                crate::util::json_type::JsonType::from_value(value)
                    .as_sql_str()
                    .to_string(),
            )
        })
        .collect();
    type_pairs.sort_unstable();
    let type_set_json = serde_json::to_string(&type_pairs).expect("type pairs serialize");
    let type_set_hash = crate::util::hash::stable_hex(type_set_json.as_bytes());

    let shape_input = format!(
        "{canonical_path}\x1f{}\x1f{schema_path}\x1f{field_set_hash}\x1f{type_set_hash}",
        site_path.unwrap_or("")
    );
    let shape_id = crate::util::hash::stable_hex(shape_input.as_bytes());

    ShapeFacts {
        shape_id,
        field_set_hash,
        type_set_hash,
        field_set_json,
        type_set_json,
    }
}
