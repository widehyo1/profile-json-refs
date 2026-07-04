use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteContext {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePresenceShapeSeed {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub field_names: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct RefsIndex {
    pub schema_by_canonical: HashMap<String, String>,
    pub site_by_source_path: HashMap<String, SiteContext>,
    pub presence_shape_seeds: Vec<SitePresenceShapeSeed>,
    pub presence_shape_truncated: bool,
}
