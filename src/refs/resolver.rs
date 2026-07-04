use crate::scan::path::SourcePath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedObjectContext {
    pub canonical_path: String,
    pub site_path: Option<String>,
    pub schema_path: String,
    pub resolved: bool,
}

pub struct RefsResolver {
    index: crate::refs::site::RefsIndex,
}

impl RefsResolver {
    pub fn new(index: crate::refs::site::RefsIndex) -> Self {
        Self { index }
    }

    pub fn resolve_object(&self, path: &SourcePath) -> ResolvedObjectContext {
        let source_path = path.as_str();
        if let Some(site) = self.index.site_by_source_path.get(&source_path) {
            return ResolvedObjectContext {
                canonical_path: site.canonical_path.clone(),
                site_path: site.site_path.clone(),
                schema_path: site.schema_path.clone(),
                resolved: true,
            };
        }

        ResolvedObjectContext {
            canonical_path: path.to_canonical_guess(),
            site_path: Some(source_path),
            schema_path: "unknown".to_string(),
            resolved: false,
        }
    }
}
