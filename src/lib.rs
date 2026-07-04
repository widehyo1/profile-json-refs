pub mod cli;
pub mod config;
pub mod error;
pub mod field;
pub mod perf;
pub mod refs;
pub mod scan;
pub mod shape;
pub mod sketch;
pub mod sqlite;
pub mod util;
pub mod value;

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{Map, Value};

use crate::config::{InputFormat, ProfileConfig};
use crate::error::Result;
use crate::field::accumulator::{FieldValueAccumulator, ShapeFieldRow};
use crate::perf::timer::{PerfBucket, PerfDestination, PerfLog};
use crate::refs::resolver::RefsResolver;
use crate::scan::path::SourcePath;
use crate::scan::visitor::ScanVisitor;
use crate::shape::accumulator::ShapeAccumulator;
use crate::sqlite::writer::{ProfileChunk, ProfileWriter, SourceCounters};
use crate::util::json_type::JsonType;

pub struct ProfileReport {
    pub out_path: PathBuf,
    pub summary: SourceSummary,
    pub elapsed: Duration,
    pub warnings: Vec<ProfileWarning>,
    pub perf_buckets: Vec<PerfBucket>,
    pub perf_enabled: bool,
    pub perf_log_file: Option<PathBuf>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileWarning {
    pub code: String,
    pub message: String,
}

pub const W_CANONICAL_PATH_UNAVAILABLE: &str = "W_CANONICAL_PATH_UNAVAILABLE";

pub fn run(config: ProfileConfig) -> Result<ProfileReport> {
    let start = Instant::now();
    config.validate()?;
    let perf_destination = config
        .perf_log_file
        .clone()
        .map(PerfDestination::File)
        .unwrap_or(PerfDestination::Stderr);
    let mut perf_log = PerfLog::new(config.perf_log, perf_destination)?;

    let resolved_format = resolve_input_format(&config);
    let source_format = resolved_format.as_source_summary_str();
    let refs_conn = perf_log.time_result("refs.open", || Connection::open(&config.refs_sqlite))?;
    let loaded_refs = perf_log.time_result("refs.load_contract", || {
        crate::refs::sqlite::load_refs_index(&refs_conn)
    })?;
    let resolver = RefsResolver::new(loaded_refs.index);
    let writer = perf_log.time_result("sqlite.create_schema", || {
        ProfileWriter::open(&config.out_sqlite, &config)
    })?;
    let quiet = config.quiet;
    let out_path = config.out_sqlite.clone();

    let mut visitor =
        ProfileRunVisitor::new(config, resolver, writer, loaded_refs.warnings, perf_log);
    let file = File::open(&visitor.config.input_file)?;
    let scan_start = Instant::now();
    match resolved_format {
        ResolvedInputFormat::Json => {
            crate::scan::json::scan_json_file(BufReader::new(file), &mut visitor)?;
        }
        ResolvedInputFormat::Jsonl => {
            crate::scan::jsonl::scan_jsonl_file(BufReader::new(file), &mut visitor)?;
        }
    }
    visitor.emit_scan_progress();
    visitor.record_perf("scan.read_parse_walk", scan_start.elapsed());

    visitor.finish(source_format, out_path, quiet, start.elapsed())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedInputFormat {
    Json,
    Jsonl,
}

impl ResolvedInputFormat {
    fn as_source_summary_str(self) -> &'static str {
        match self {
            ResolvedInputFormat::Json => "json",
            ResolvedInputFormat::Jsonl => "jsonl",
        }
    }
}

fn resolve_input_format(config: &ProfileConfig) -> ResolvedInputFormat {
    match config.input_format {
        InputFormat::Json => ResolvedInputFormat::Json,
        InputFormat::Jsonl => ResolvedInputFormat::Jsonl,
        InputFormat::Auto => {
            if config
                .input_file
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
            {
                ResolvedInputFormat::Jsonl
            } else {
                ResolvedInputFormat::Json
            }
        }
    }
}

struct ProfileRunVisitor {
    config: ProfileConfig,
    resolver: RefsResolver,
    writer: ProfileWriter,
    counters: SourceCounters,
    shape_accumulator: ShapeAccumulator,
    shape_fields: HashMap<String, ShapeFieldRow>,
    field_values: HashMap<String, FieldValueAccumulator>,
    warnings: Vec<ProfileWarning>,
    warned_canonical_path_unavailable: bool,
    perf_log: PerfLog,
}

impl ProfileRunVisitor {
    fn new(
        config: ProfileConfig,
        resolver: RefsResolver,
        writer: ProfileWriter,
        warnings: Vec<ProfileWarning>,
        perf_log: PerfLog,
    ) -> Self {
        Self {
            config,
            resolver,
            writer,
            counters: SourceCounters::default(),
            shape_accumulator: ShapeAccumulator::default(),
            shape_fields: HashMap::new(),
            field_values: HashMap::new(),
            warnings,
            warned_canonical_path_unavailable: false,
            perf_log,
        }
    }

    fn record_perf(&mut self, name: &'static str, duration: Duration) {
        self.perf_log.record(name, duration);
    }

    fn emit_scan_progress(&mut self) {
        self.perf_log.event(&format!(
            "phase=scan.progress documents={} objects={} arrays={} scalars={}",
            self.counters.total_document_count,
            self.counters.total_object_count,
            self.counters.total_array_count,
            self.counters.total_scalar_count
        ));
    }

    fn emit_flush_chunk(&mut self, chunk: &ProfileChunk) {
        self.perf_log.event(&format!(
            "phase=flush.chunk shapes={} fields={} object_samples={} value_samples={}",
            chunk.shapes.len(),
            chunk.shape_fields.len(),
            chunk.object_samples.len(),
            chunk.value_samples.len()
        ));
    }

    fn emit_dbstat(&mut self) {
        match self.writer.dbstat_summary() {
            Some(summary) => self.perf_log.event(&format!(
                "phase=sqlite.dbstat top_table={} mb={:.3}",
                summary.top_table, summary.mb
            )),
            None => self.perf_log.event("phase=sqlite.dbstat unavailable=1"),
        }
    }

    fn finish(
        mut self,
        source_format: &str,
        out_path: PathBuf,
        quiet: bool,
        elapsed: Duration,
    ) -> Result<ProfileReport> {
        self.flush_pending_samples()?;

        let value_start = Instant::now();
        let mut field_outputs: Vec<_> = std::mem::take(&mut self.field_values)
            .into_values()
            .map(|accumulator| accumulator.finish(&self.config))
            .collect();
        self.perf_log
            .record("scan.flush_values", value_start.elapsed());
        field_outputs.sort_by(|left, right| {
            left.summary
                .field_profile_id
                .cmp(&right.summary.field_profile_id)
        });

        let mut final_chunk = self.drain_pending_chunk();
        for output in field_outputs {
            final_chunk.field_summaries.push(output.summary);
            final_chunk.field_values.extend(output.field_values);
            final_chunk.value_samples.extend(output.value_samples);
        }
        self.flush_chunk(final_chunk)?;
        self.perf_log.record("sqlite.prune_samples", Duration::ZERO);
        self.perf_log
            .time_result("sqlite.indexes", || self.writer.create_indexes())?;
        let summary = self
            .writer
            .write_source_summary(source_format, self.counters)?;
        if self.config.perf_log_dbstat {
            self.emit_dbstat();
        }
        self.perf_log.record("total", elapsed);
        let perf_enabled = self.config.perf_log;
        let perf_log_file = self.config.perf_log_file.clone();
        let perf_buckets = self.perf_log.into_buckets();

        Ok(ProfileReport {
            out_path,
            summary,
            elapsed,
            warnings: self.warnings,
            perf_buckets,
            perf_enabled,
            perf_log_file,
            quiet,
        })
    }

    fn observe_object_fields(
        &mut self,
        document_index: u64,
        object_source_path: &str,
        shape_id: &str,
        object: &Map<String, Value>,
    ) {
        let parent_object = Value::Object(object.clone());

        for (field_name, value) in object {
            let observed_type = JsonType::from_value(value);
            let field_profile_id =
                crate::field::id::field_profile_id(shape_id, field_name, observed_type);
            let row = self
                .shape_fields
                .entry(field_profile_id.clone())
                .or_insert_with(|| ShapeFieldRow {
                    field_profile_id: field_profile_id.clone(),
                    shape_id: shape_id.to_string(),
                    field_name: field_name.clone(),
                    observed_type,
                    observed_count: 0,
                    null_count: 0,
                });
            row.observed_count += 1;
            if matches!(value, Value::Null) {
                row.null_count += 1;
            }

            let source_path = field_source_path(object_source_path, field_name);
            self.field_values
                .entry(field_profile_id.clone())
                .or_insert_with(|| FieldValueAccumulator::new(field_profile_id, &self.config))
                .observe(
                    document_index,
                    &source_path,
                    value,
                    &parent_object,
                    &self.config,
                );
        }
    }

    fn flush_if_needed(&mut self) -> Result<()> {
        if self.shape_accumulator.shape_row_count() >= self.config.flush.chunk_shape_rows
            || self.shape_accumulator.pending_object_sample_count()
                >= self.config.flush.chunk_object_sample_rows
            || self.shape_fields.len() >= self.config.flush.chunk_field_rows
            || self.pending_value_sample_count() >= self.config.flush.chunk_value_sample_rows
        {
            self.flush_pending_samples()?;
        }
        Ok(())
    }

    fn flush_pending_samples(&mut self) -> Result<()> {
        let chunk = self.drain_pending_chunk();
        self.flush_chunk(chunk)
    }

    fn flush_chunk(&mut self, chunk: ProfileChunk) -> Result<()> {
        if !chunk.is_empty() {
            self.emit_flush_chunk(&chunk);
        }
        self.writer.flush_chunk(chunk)
    }

    fn drain_pending_chunk(&mut self) -> ProfileChunk {
        let shape_start = Instant::now();
        let shapes = self.shape_accumulator.drain_shape_rows();
        self.perf_log
            .record("scan.flush_shapes", shape_start.elapsed());

        let field_start = Instant::now();
        let mut shape_fields: Vec<_> = self.shape_fields.drain().map(|(_, row)| row).collect();
        shape_fields.sort_by(|left, right| left.field_profile_id.cmp(&right.field_profile_id));
        self.perf_log
            .record("scan.flush_fields", field_start.elapsed());

        let sample_start = Instant::now();
        let object_samples = self.shape_accumulator.drain_object_sample_rows();
        let value_samples = self.drain_value_sample_rows();
        self.perf_log
            .record("scan.flush_samples", sample_start.elapsed());

        ProfileChunk {
            shapes,
            shape_fields,
            object_samples,
            field_summaries: Vec::new(),
            field_values: Vec::new(),
            value_samples,
        }
    }

    fn drain_value_sample_rows(&mut self) -> Vec<crate::value::sample::ValueSampleRow> {
        let mut field_profile_ids: Vec<_> = self.field_values.keys().cloned().collect();
        field_profile_ids.sort();

        let mut rows = Vec::new();
        for field_profile_id in field_profile_ids {
            if let Some(accumulator) = self.field_values.get_mut(&field_profile_id) {
                rows.extend(accumulator.drain_value_sample_rows());
            }
        }
        rows
    }

    fn pending_value_sample_count(&self) -> usize {
        self.field_values
            .values()
            .map(FieldValueAccumulator::pending_value_sample_count)
            .sum()
    }

    fn warn_unresolved_context_once(&mut self, path: &SourcePath) {
        if self.warned_canonical_path_unavailable {
            return;
        }
        self.warned_canonical_path_unavailable = true;
        self.warnings.push(ProfileWarning {
            code: W_CANONICAL_PATH_UNAVAILABLE.to_string(),
            message: format!(
                "refs database did not contain an exact context for source path {}",
                path.as_str()
            ),
        });
    }
}

impl ScanVisitor for ProfileRunVisitor {
    fn begin_document(&mut self, _document_index: u64) -> Result<()> {
        self.counters.total_document_count += 1;
        Ok(())
    }

    fn end_document(&mut self, _document_index: u64) -> Result<()> {
        self.flush_if_needed()
    }

    fn visit_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        object: &Map<String, Value>,
    ) -> Result<()> {
        self.counters.total_object_count += 1;
        let context = self.resolver.resolve_object(path);
        if !context.resolved {
            self.warn_unresolved_context_once(path);
        }
        let facts = self.shape_accumulator.observe_object_with_facts(
            document_index,
            path,
            &context,
            object,
            &self.config.sampling,
        )?;
        self.observe_object_fields(document_index, &path.as_str(), &facts.shape_id, object);
        self.flush_if_needed()
    }

    fn visit_array(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _array: &[Value],
    ) -> Result<()> {
        self.counters.total_array_count += 1;
        Ok(())
    }

    fn visit_scalar(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _value: &Value,
    ) -> Result<()> {
        self.counters.total_scalar_count += 1;
        Ok(())
    }
}

fn field_source_path(object_source_path: &str, field_name: &str) -> String {
    if object_source_path == "$" {
        format!("$.{field_name}")
    } else {
        format!("{object_source_path}.{field_name}")
    }
}
