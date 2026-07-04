use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::CliArgs;
use crate::error::{ProfileError, Result};

#[derive(Debug, Clone)]
pub struct ProfileConfig {
    pub input_file: PathBuf,
    pub refs_sqlite: PathBuf,
    pub out_sqlite: PathBuf,
    pub input_format: InputFormat,
    pub quiet: bool,
    pub perf_log: bool,
    pub perf_log_file: Option<PathBuf>,
    pub perf_log_dbstat: bool,
    pub sampling: SamplingConfig,
    pub value_profile: ValueProfileConfig,
    pub flush: FlushConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Auto,
    Json,
    Jsonl,
}

#[derive(Debug, Clone)]
pub struct SamplingConfig {
    pub object_json_limit_bytes: usize,
    pub canonical_priority_limit: usize,
    pub site_priority_limit: usize,
    pub field_set_priority_limit: usize,
    pub type_set_priority_limit: usize,
    pub value_json_limit_bytes: usize,
    pub parent_object_json_limit_bytes: usize,
    pub value_priority_limit_per_field_profile: usize,
    pub heavy_hitter_context_sample_limit: usize,
}

#[derive(Debug, Clone)]
pub struct ValueProfileConfig {
    pub value_text_limit_bytes: usize,
    pub exact_distinct_threshold: usize,
    pub exact_value_bytes_per_field_profile: usize,
    pub global_exact_value_bytes_budget: usize,
    pub hll_precision: u8,
    pub heavy_hitter_limit: usize,
}

#[derive(Debug, Clone)]
pub struct FlushConfig {
    pub chunk_object_sample_rows: usize,
    pub chunk_value_sample_rows: usize,
    pub chunk_shape_rows: usize,
    pub chunk_field_rows: usize,
}

impl ProfileConfig {
    pub fn from_cli(args: CliArgs) -> Result<Self> {
        let mut config = Self {
            input_file: args.input_file.clone(),
            refs_sqlite: default_refs_sqlite(),
            out_sqlite: default_out_sqlite(),
            input_format: InputFormat::Auto,
            quiet: false,
            perf_log: false,
            perf_log_file: None,
            perf_log_dbstat: false,
            sampling: SamplingConfig::default(),
            value_profile: ValueProfileConfig::default(),
            flush: FlushConfig::default(),
        };

        if let Some(config_path) = &args.config {
            let file_config = load_file_config(config_path)?;
            config.apply_file_config(file_config)?;
        }

        config.apply_cli_overrides(args);
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.input_file.as_os_str() == "-" {
            return Err(ProfileError::StdinUnsupported);
        }
        if !self.input_file.is_file() {
            return Err(ProfileError::InputNotFile(self.input_file.clone()));
        }
        if !self.refs_sqlite.is_file() {
            return Err(ProfileError::RefsNotFile(self.refs_sqlite.clone()));
        }
        if !(4..=18).contains(&self.value_profile.hll_precision) {
            return Err(ProfileError::InvalidConfig(
                "hll_precision must be between 4 and 18".into(),
            ));
        }
        if self.value_profile.heavy_hitter_limit == 0 {
            return Err(ProfileError::InvalidConfig(
                "heavy_hitter_limit must be > 0".into(),
            ));
        }
        if self.value_profile.exact_distinct_threshold <= self.value_profile.heavy_hitter_limit {
            return Err(ProfileError::InvalidConfig(
                "exact_distinct_threshold should be greater than heavy_hitter_limit".into(),
            ));
        }
        ensure_positive(
            self.sampling.type_set_priority_limit,
            "sampling.object.type_set.priority_sample_limit",
        )?;
        ensure_positive(
            self.sampling.value_priority_limit_per_field_profile,
            "sampling.value.priority_sample_limit_per_field_profile",
        )?;
        ensure_positive(
            self.sampling.object_json_limit_bytes,
            "object sample JSON limit",
        )?;
        ensure_positive(
            self.sampling.value_json_limit_bytes,
            "value sample JSON limit",
        )?;
        ensure_positive(
            self.sampling.parent_object_json_limit_bytes,
            "parent object sample JSON limit",
        )?;
        ensure_positive(
            self.value_profile.value_text_limit_bytes,
            "value_profile.value_text_limit_bytes",
        )?;
        ensure_positive(
            self.value_profile.exact_value_bytes_per_field_profile,
            "value_profile.exact_value_bytes_per_field_profile",
        )?;
        ensure_positive(
            self.value_profile.global_exact_value_bytes_budget,
            "value_profile.global_exact_value_bytes_budget",
        )?;
        ensure_positive(
            self.flush.chunk_object_sample_rows,
            "flush.chunk_object_sample_rows",
        )?;
        ensure_positive(
            self.flush.chunk_value_sample_rows,
            "flush.chunk_value_sample_rows",
        )?;
        ensure_positive(self.flush.chunk_shape_rows, "flush.chunk_shape_rows")?;
        ensure_positive(self.flush.chunk_field_rows, "flush.chunk_field_rows")?;
        Ok(())
    }

    fn apply_file_config(&mut self, file: FileConfig) -> Result<()> {
        if let Some(input) = file.input {
            let _ = input.file;
            if let Some(format) = input.format {
                self.input_format = parse_input_format(&format)?;
            }
        }

        if let Some(refs) = file.refs
            && let Some(sqlite) = refs.sqlite
        {
            self.refs_sqlite = sqlite;
        }

        if let Some(output) = file.output
            && let Some(sqlite) = output.sqlite
        {
            self.out_sqlite = sqlite;
        }

        if let Some(stdout) = file.stdout
            && let Some(quiet) = stdout.quiet
        {
            self.quiet = quiet;
        }

        if let Some(perf) = file.perf {
            if let Some(log) = perf.log {
                self.perf_log = log;
            }
            if let Some(file) = perf.file {
                self.perf_log_file = Some(file);
                self.perf_log = true;
            }
            if let Some(dbstat) = perf.dbstat {
                self.perf_log_dbstat = dbstat;
            }
        }

        if let Some(sampling) = file.sampling {
            self.apply_sampling_file_config(sampling);
        }

        if let Some(value_profile) = file.value_profile {
            self.apply_value_profile_file_config(value_profile);
        }

        if let Some(flush) = file.flush {
            self.apply_flush_file_config(flush);
        }

        Ok(())
    }

    fn apply_sampling_file_config(&mut self, sampling: SamplingFileConfig) {
        if let Some(object) = sampling.object {
            if let Some(limit) = object.sample_json_limit_bytes {
                self.sampling.object_json_limit_bytes = limit;
            }
            if let Some(rows) = object.chunk_flush_rows {
                self.flush.chunk_object_sample_rows = rows;
            }
            if let Some(limit) = object.canonical_path.and_then(priority_limit) {
                self.sampling.canonical_priority_limit = limit;
            }
            if let Some(limit) = object.site_path.and_then(priority_limit) {
                self.sampling.site_priority_limit = limit;
            }
            if let Some(limit) = object.field_set.and_then(priority_limit) {
                self.sampling.field_set_priority_limit = limit;
            }
            if let Some(limit) = object.type_set.and_then(priority_limit) {
                self.sampling.type_set_priority_limit = limit;
            }
        }

        if let Some(value) = sampling.value {
            let _ = (value.first_seen, value.first_non_empty);
            if let Some(limit) = value.value_json_limit_bytes {
                self.sampling.value_json_limit_bytes = limit;
            }
            if let Some(limit) = value.parent_object_json_limit_bytes {
                self.sampling.parent_object_json_limit_bytes = limit;
            }
            if let Some(rows) = value.chunk_flush_rows {
                self.flush.chunk_value_sample_rows = rows;
            }
            if let Some(limit) = value.priority_sample_limit_per_field_profile {
                self.sampling.value_priority_limit_per_field_profile = limit;
            }
            if let Some(limit) = value.heavy_hitter_context_sample_limit {
                self.sampling.heavy_hitter_context_sample_limit = limit;
            }
        }
    }

    fn apply_value_profile_file_config(&mut self, value_profile: ValueProfileFileConfig) {
        if let Some(limit) = value_profile.value_text_limit_bytes {
            self.value_profile.value_text_limit_bytes = limit;
        }
        if let Some(threshold) = value_profile.exact_distinct_threshold {
            self.value_profile.exact_distinct_threshold = threshold;
        }
        if let Some(limit) = value_profile.exact_value_bytes_per_field_profile {
            self.value_profile.exact_value_bytes_per_field_profile = limit;
        }
        if let Some(limit) = value_profile.global_exact_value_bytes_budget {
            self.value_profile.global_exact_value_bytes_budget = limit;
        }
        if let Some(precision) = value_profile.hll_precision {
            self.value_profile.hll_precision = precision;
        }
        if let Some(limit) = value_profile.heavy_hitter_limit {
            self.value_profile.heavy_hitter_limit = limit;
        }
    }

    fn apply_flush_file_config(&mut self, flush: FlushFileConfig) {
        if let Some(rows) = flush.chunk_object_sample_rows {
            self.flush.chunk_object_sample_rows = rows;
        }
        if let Some(rows) = flush.chunk_value_sample_rows {
            self.flush.chunk_value_sample_rows = rows;
        }
        if let Some(rows) = flush.chunk_shape_rows {
            self.flush.chunk_shape_rows = rows;
        }
        if let Some(rows) = flush.chunk_field_rows {
            self.flush.chunk_field_rows = rows;
        }
    }

    fn apply_cli_overrides(&mut self, args: CliArgs) {
        if let Some(refs) = args.refs {
            self.refs_sqlite = refs;
        }
        if let Some(out) = args.out {
            self.out_sqlite = out;
        }
        if args.jsonl {
            self.input_format = InputFormat::Jsonl;
        }
        if let Some(limit) = args.shape_sample_limit {
            self.sampling.type_set_priority_limit = limit;
        }
        if let Some(limit) = args.value_sample_limit {
            self.sampling.value_priority_limit_per_field_profile = limit;
        }
        if let Some(limit) = args.heavy_hitter_limit {
            self.value_profile.heavy_hitter_limit = limit;
        }
        if let Some(precision) = args.hll_precision {
            self.value_profile.hll_precision = precision;
        }
        if let Some(limit) = args.value_text_limit {
            self.value_profile.value_text_limit_bytes = limit;
        }
        if args.quiet {
            self.quiet = true;
        }
        if args.perf_log {
            self.perf_log = true;
        }
        if let Some(file) = args.perf_log_file {
            self.perf_log_file = Some(file);
            self.perf_log = true;
        }
        if args.perf_log_dbstat {
            self.perf_log_dbstat = true;
        }
    }
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            object_json_limit_bytes: 16 * 1024,
            canonical_priority_limit: 1,
            site_priority_limit: 1,
            field_set_priority_limit: 2,
            type_set_priority_limit: 4,
            value_json_limit_bytes: 1024,
            parent_object_json_limit_bytes: 1024,
            value_priority_limit_per_field_profile: 4,
            heavy_hitter_context_sample_limit: 0,
        }
    }
}

impl Default for ValueProfileConfig {
    fn default() -> Self {
        Self {
            value_text_limit_bytes: 512,
            exact_distinct_threshold: 4096,
            exact_value_bytes_per_field_profile: 1024 * 1024,
            global_exact_value_bytes_budget: 256 * 1024 * 1024,
            hll_precision: 14,
            heavy_hitter_limit: 128,
        }
    }
}

impl Default for FlushConfig {
    fn default() -> Self {
        Self {
            chunk_object_sample_rows: 10_000,
            chunk_value_sample_rows: 10_000,
            chunk_shape_rows: 10_000,
            chunk_field_rows: 25_000,
        }
    }
}

fn default_refs_sqlite() -> PathBuf {
    PathBuf::from("refs/schemas.sqlite")
}

fn default_out_sqlite() -> PathBuf {
    PathBuf::from("profile.sqlite")
}

fn load_file_config(path: &Path) -> Result<FileConfig> {
    let yaml = fs::read_to_string(path).map_err(|source| ProfileError::ConfigRead {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&yaml).map_err(|source| ProfileError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_input_format(raw: &str) -> Result<InputFormat> {
    match raw.to_ascii_lowercase().as_str() {
        "auto" => Ok(InputFormat::Auto),
        "json" => Ok(InputFormat::Json),
        "jsonl" => Ok(InputFormat::Jsonl),
        other => Err(ProfileError::InvalidConfig(format!(
            "unsupported input.format: {other}"
        ))),
    }
}

fn ensure_positive(value: usize, label: &str) -> Result<()> {
    if value == 0 {
        return Err(ProfileError::InvalidConfig(format!("{label} must be > 0")));
    }
    Ok(())
}

fn priority_limit(config: PrioritySampleFileConfig) -> Option<usize> {
    let _ = (config.first_seen, config.first_non_empty);
    config.priority_sample_limit
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    input: Option<InputConfig>,
    refs: Option<RefsConfig>,
    output: Option<OutputConfig>,
    stdout: Option<StdoutConfig>,
    perf: Option<PerfConfig>,
    sampling: Option<SamplingFileConfig>,
    value_profile: Option<ValueProfileFileConfig>,
    flush: Option<FlushFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InputConfig {
    file: Option<PathBuf>,
    format: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RefsConfig {
    sqlite: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct OutputConfig {
    sqlite: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StdoutConfig {
    quiet: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PerfConfig {
    log: Option<bool>,
    file: Option<PathBuf>,
    dbstat: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SamplingFileConfig {
    object: Option<ObjectSamplingFileConfig>,
    value: Option<ValueSamplingFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ObjectSamplingFileConfig {
    sample_json_limit_bytes: Option<usize>,
    chunk_flush_rows: Option<usize>,
    canonical_path: Option<PrioritySampleFileConfig>,
    site_path: Option<PrioritySampleFileConfig>,
    field_set: Option<PrioritySampleFileConfig>,
    type_set: Option<PrioritySampleFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct PrioritySampleFileConfig {
    first_seen: Option<bool>,
    first_non_empty: Option<bool>,
    priority_sample_limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ValueSamplingFileConfig {
    value_json_limit_bytes: Option<usize>,
    parent_object_json_limit_bytes: Option<usize>,
    chunk_flush_rows: Option<usize>,
    first_seen: Option<bool>,
    first_non_empty: Option<bool>,
    priority_sample_limit_per_field_profile: Option<usize>,
    heavy_hitter_context_sample_limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ValueProfileFileConfig {
    value_text_limit_bytes: Option<usize>,
    exact_distinct_threshold: Option<usize>,
    exact_value_bytes_per_field_profile: Option<usize>,
    global_exact_value_bytes_budget: Option<usize>,
    hll_precision: Option<u8>,
    heavy_hitter_limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FlushFileConfig {
    chunk_object_sample_rows: Option<usize>,
    chunk_value_sample_rows: Option<usize>,
    chunk_shape_rows: Option<usize>,
    chunk_field_rows: Option<usize>,
}
