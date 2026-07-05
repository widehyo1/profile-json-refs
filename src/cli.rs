use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "profile-json-refs",
    version,
    about = "Profile JSON/JSONL values using dump-json-refs structural refs"
)]
pub struct CliArgs {
    /// Input JSON/JSONL file. Stdin is not supported in v0.1.0.
    pub input_file: PathBuf,

    /// Path to refs/schemas.sqlite produced by dump-json-refs.
    #[arg(long = "refs", value_name = "FILE")]
    pub refs: Option<PathBuf>,

    /// Output profile.sqlite path.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Force JSONL mode.
    #[arg(long)]
    pub jsonl: bool,

    /// YAML config file.
    #[arg(long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Type-set object priority sample limit.
    #[arg(long = "shape-sample-limit", value_name = "N")]
    pub shape_sample_limit: Option<usize>,

    /// Value priority sample limit per field profile.
    #[arg(long = "value-sample-limit", value_name = "N")]
    pub value_sample_limit: Option<usize>,

    /// Space-Saving candidate limit.
    #[arg(long = "heavy-hitter-limit", value_name = "N")]
    pub heavy_hitter_limit: Option<usize>,

    /// HyperLogLog precision.
    #[arg(long = "hll-precision", value_name = "N")]
    pub hll_precision: Option<u8>,

    /// Display/storage limit for prof_field_value.value_text.
    #[arg(long = "value-text-limit", value_name = "BYTES")]
    pub value_text_limit: Option<usize>,

    /// Emit timing buckets to stderr.
    #[arg(long = "perf-log")]
    pub perf_log: bool,

    /// Write perf-log events to a file instead of stderr.
    #[arg(long = "perf-log-file", value_name = "FILE")]
    pub perf_log_file: Option<PathBuf>,

    /// Include optional SQLite dbstat diagnostics in perf-log output.
    #[arg(long = "perf-log-dbstat")]
    pub perf_log_dbstat: bool,

    /// Suppress normal stdout on success.
    #[arg(long)]
    pub quiet: bool,
}
