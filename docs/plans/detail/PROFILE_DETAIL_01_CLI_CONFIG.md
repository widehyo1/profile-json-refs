# Profile Detail 01: CLI and Config

Covers:

```text
Phase 1: CLI and config contract
```

---

## 1. Target Files

```text
src/main.rs
src/lib.rs
src/cli.rs
src/config.rs
src/error.rs
tests/cli_contract.rs
fixtures/config/
```

---

## 2. Dependencies

Recommended crates:

```toml
[dependencies]
anyhow = "1"
thiserror = "2"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
rusqlite = { version = "0.32", features = ["bundled"] }
serde_json = "1"
```

Optional later:

```toml
twox-hash = "2"
```

Use a stable hash wrapper in `src/util/hash.rs` so hash implementation can change without changing call sites.

---

## 3. CLI Shape

`src/cli.rs`:

```rust
use std::path::PathBuf;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "profile-json-refs")]
#[command(about = "Profile JSON/JSONL values using dump-json-refs structural refs")]
pub struct CliArgs {
    /// Input JSON/JSONL file. Stdin is not supported in v0.1.0.
    pub input_file: PathBuf,

    /// Path to refs/schemas.sqlite produced by dump-json-refs.
    #[arg(long = "refs", default_value = "refs/schemas.sqlite")]
    pub refs: PathBuf,

    /// Output profile.sqlite path.
    #[arg(short = 'o', long = "out", default_value = "profile.sqlite")]
    pub out: PathBuf,

    /// Force JSONL mode.
    #[arg(long)]
    pub jsonl: bool,

    /// YAML config file.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Type-set object priority sample limit.
    #[arg(long = "shape-sample-limit")]
    pub shape_sample_limit: Option<usize>,

    /// Value priority sample limit per field profile.
    #[arg(long = "value-sample-limit")]
    pub value_sample_limit: Option<usize>,

    /// Space-Saving candidate limit.
    #[arg(long = "heavy-hitter-limit")]
    pub heavy_hitter_limit: Option<usize>,

    /// HyperLogLog precision.
    #[arg(long = "hll-precision")]
    pub hll_precision: Option<u8>,

    /// Display/storage limit for prof_field_value.value_text.
    #[arg(long = "value-text-limit")]
    pub value_text_limit: Option<usize>,

    /// Emit timing and progress events. Default destination: stderr.
    #[arg(long = "perf-log")]
    pub perf_log: bool,

    /// Write perf-log events to a file instead of stderr.
    #[arg(long = "perf-log-file")]
    pub perf_log_file: Option<PathBuf>,

    /// Include optional SQLite dbstat diagnostics in perf-log output.
    #[arg(long = "perf-log-dbstat")]
    pub perf_log_dbstat: bool,

    /// Suppress normal stdout on success.
    #[arg(long)]
    pub quiet: bool,
}
```

Do not define `--strict`.

---

## 4. Main Entry Point

`src/main.rs`:

```rust
use clap::Parser;
use profile_json_refs::{cli::CliArgs, config::ProfileConfig};

fn main() {
    let args = CliArgs::parse();

    match ProfileConfig::from_cli(args).and_then(profile_json_refs::run) {
        Ok(report) => {
            if !report.summary_quiet() {
                print!("{}", report.to_stdout_summary());
            }
        }
        Err(err) => {
            eprintln!("ERROR {err}");
            std::process::exit(1);
        }
    }
}
```

The exact formatting helper can live in Phase 10. In Phase 1, it is acceptable for `main` to parse config and exit.

---

## 5. Config Model

`src/config.rs`:

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ProfileConfig {
    pub input_file: PathBuf,
    pub refs_sqlite: PathBuf,
    pub out_sqlite: PathBuf,
    pub input_format: InputFormat,
    pub quiet: bool,
    pub perf_log: PerfLogConfig,
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
pub struct PerfLogConfig {
    pub enabled: bool,
    pub file: Option<PathBuf>,
    pub dbstat: bool,
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
```

Default values:

```rust
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
```

---

## 6. YAML Config Shape

Use `deny_unknown_fields` on deserialized config fragments.

```rust
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub input: Option<InputConfig>,
    pub refs: Option<RefsConfig>,
    pub output: Option<OutputConfig>,
    pub sampling: Option<SamplingFileConfig>,
    pub value_profile: Option<ValueProfileFileConfig>,
    pub flush: Option<FlushFileConfig>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InputConfig {
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RefsConfig {
    pub sqlite: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    pub sqlite: Option<PathBuf>,
}
```

YAML example:

```yaml
input:
  format: auto

perf:
  log: false
  file: null
  dbstat: false

refs:
  sqlite: refs/schemas.sqlite

output:
  sqlite: profile.sqlite

sampling:
  object:
    sample_json_limit_bytes: 16384
    canonical_path:
      priority_sample_limit: 1
    site_path:
      priority_sample_limit: 1
    field_set:
      priority_sample_limit: 2
    type_set:
      priority_sample_limit: 4
  value:
    value_json_limit_bytes: 1024
    parent_object_json_limit_bytes: 1024
    priority_sample_limit_per_field_profile: 4
    heavy_hitter_context_sample_limit: 0

value_profile:
  value_text_limit_bytes: 512
  exact_distinct_threshold: 4096
  exact_value_bytes_per_field_profile: 1048576
  global_exact_value_bytes_budget: 268435456
  hll_precision: 14
  heavy_hitter_limit: 128

flush:
  chunk_object_sample_rows: 10000
  chunk_value_sample_rows: 10000
```

---

## 7. Precedence

Precedence order:

```text
defaults < YAML config < explicit CLI flags
```

The positional `input_file` is always required by CLI. v0.1.0 does not support reading the source input path from YAML.

CLI mapping:

```text
--refs                         -> refs.sqlite
--out                          -> output.sqlite
--jsonl                        -> input.format = jsonl
--shape-sample-limit <N>       -> sampling.object.type_set.priority_sample_limit
--value-sample-limit <N>       -> sampling.value.priority_sample_limit_per_field_profile
--heavy-hitter-limit <N>       -> value_profile.heavy_hitter_limit
--hll-precision <N>            -> value_profile.hll_precision
--value-text-limit <BYTES>     -> value_profile.value_text_limit_bytes
--quiet                        -> runtime quiet flag
--perf-log                     -> perf.enabled = true
--perf-log-file <FILE>          -> perf.file
--perf-log-dbstat               -> perf.dbstat = true
```

---

## 8. Validation

Validation rules:

```rust
impl ProfileConfig {
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.input_file.as_os_str() == "-" {
            return Err(ProfileError::StdinUnsupported.into());
        }
        if !self.input_file.is_file() {
            return Err(ProfileError::InputNotFile(self.input_file.clone()).into());
        }
        if !self.refs_sqlite.is_file() {
            return Err(ProfileError::RefsNotFile(self.refs_sqlite.clone()).into());
        }
        if !(4..=18).contains(&self.value_profile.hll_precision) {
            return Err(ProfileError::InvalidConfig("hll_precision must be between 4 and 18".into()).into());
        }
        if self.value_profile.heavy_hitter_limit == 0 {
            return Err(ProfileError::InvalidConfig("heavy_hitter_limit must be > 0".into()).into());
        }
        // heavy_hitter_context_sample_limit may be 0 in rc.2.
        // 0 means heavy hitter context samples are disabled by default.
        if self.value_profile.exact_distinct_threshold <= self.value_profile.heavy_hitter_limit {
            return Err(ProfileError::InvalidConfig(
                "exact_distinct_threshold should be greater than heavy_hitter_limit".into()
            ).into());
        }
        Ok(())
    }
}
```

`--shape-sample-limit 0` should be invalid because type-set priority samples would be disabled while the contract expects bounded representative samples.

---

## 9. Error Type

`src/error.rs`:

```rust
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ProfileError>;

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("stdin is not supported in v0.1.0; pass a JSON or JSONL file path")]
    StdinUnsupported,

    #[error("input path is not a file: {0}")]
    InputNotFile(PathBuf),

    #[error("refs sqlite path is not a file: {0}")]
    RefsNotFile(PathBuf),

    #[error("invalid config: {0}")]
    InvalidConfig(String),

    #[error("failed to read config {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse config {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 10. Phase 1 Tests

`tests/cli_contract.rs` should verify:

```text
- default refs path is refs/schemas.sqlite
- default output path is profile.sqlite
- --jsonl forces InputFormat::Jsonl
- --strict is rejected by clap
- '-' is rejected
- stdin pipeline is not accepted
- unknown YAML key fails
- CLI overrides YAML
- heavy_hitter_context_sample_limit = 0 is valid
- --perf-log-file and --perf-log-dbstat parse
- invalid hll_precision fails
- heavy_hitter_limit = 0 fails
```

Use config-level tests where possible; full CLI process tests can come later.

---

## 11. Commit

```bash
git add src/main.rs src/lib.rs src/cli.rs src/config.rs src/error.rs tests/cli_contract.rs fixtures/config
git commit -m "feat(cli): implement config and input contract"
```
