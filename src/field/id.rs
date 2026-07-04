use crate::util::json_type::JsonType;

pub fn field_profile_id(shape_id: &str, field_name: &str, observed_type: JsonType) -> String {
    let input = format!(
        "{shape_id}\x1f{field_name}\x1f{}",
        observed_type.as_sql_str()
    );
    crate::util::hash::stable_hex(input.as_bytes())
}
