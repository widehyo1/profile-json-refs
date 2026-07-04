pub mod cli;
pub mod config;
pub mod error;
pub mod refs;
pub mod scan;
pub mod sqlite;

use std::path::PathBuf;
use std::time::Duration;

use crate::config::ProfileConfig;
use crate::error::Result;

pub struct ProfileReport {
    pub out_path: PathBuf,
    pub summary: SourceSummary,
    pub elapsed: Duration,
    pub warnings: Vec<ProfileWarning>,
    pub quiet: bool,
}

impl ProfileReport {
    pub fn summary_quiet(&self) -> bool {
        self.quiet
    }

    pub fn to_stdout_summary(&self) -> String {
        format!(
            "\
profile-json-refs: wrote {}

documents: {}
objects: {}
arrays: {}
scalars: {}
canonical_paths: {}
site_paths: {}
shapes: {}
field_profiles: {}
stored_values: {}
elapsed: {:.3}s
",
            self.out_path.display(),
            self.summary.total_document_count,
            self.summary.total_object_count,
            self.summary.total_array_count,
            self.summary.total_scalar_count,
            self.summary.total_canonical_path_count,
            self.summary.total_site_path_count,
            self.summary.total_shape_count,
            self.summary.total_field_profile_count,
            self.summary.total_stored_value_count,
            self.elapsed.as_secs_f64(),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourceSummary {
    pub total_document_count: u64,
    pub total_object_count: u64,
    pub total_array_count: u64,
    pub total_scalar_count: u64,
    pub total_canonical_path_count: u64,
    pub total_site_path_count: u64,
    pub total_shape_count: u64,
    pub total_field_profile_count: u64,
    pub total_stored_value_count: u64,
}

#[derive(Debug, Clone)]
pub struct ProfileWarning {
    pub code: String,
    pub message: String,
}

pub fn run(_config: ProfileConfig) -> Result<ProfileReport> {
    todo!("wired in Phase 9")
}
