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
use crate::field::accumulator::{FieldValueAccumulator, FieldValueObserveTiming, ShapeFieldRow};
use crate::perf::timer::{PerfBucket, PerfDestination, PerfLog};
use crate::refs::resolver::RefsResolver;
use crate::scan::path::SourcePath;
use crate::scan::visitor::ScanVisitor;
use crate::shape::accumulator::ShapeAccumulator;
use crate::sqlite::writer::{ProfileChunk, ProfileWriter, SourceCounters};
use crate::util::json_type::JsonType;
use crate::value::sample::PendingRowDelta;

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
    if !visitor.has_pending_scan_completion_flush() {
        visitor.emit_scan_chunk(ScanChunkReason::Progress);
    }
    visitor.emit_scan_accumulators();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanChunkReason {
    Progress,
    PreFlush,
    Final,
}

impl ScanChunkReason {
    fn as_str(self) -> &'static str {
        match self {
            ScanChunkReason::Progress => "progress",
            ScanChunkReason::PreFlush => "pre_flush",
            ScanChunkReason::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlushReason {
    ValueSampleLimit,
    DocumentChunkLimit,
    FinalSamples,
    FinalFieldValues,
    Explicit,
}

impl FlushReason {
    fn as_str(self) -> &'static str {
        match self {
            FlushReason::ValueSampleLimit => "value_sample_limit",
            FlushReason::DocumentChunkLimit => "document_chunk_limit",
            FlushReason::FinalSamples => "final_samples",
            FlushReason::FinalFieldValues => "final_field_values",
            FlushReason::Explicit => "explicit",
        }
    }
}

const SCAN_SAMPLED_WALK_INTERVAL: u64 = 1024;

#[derive(Debug, Clone)]
struct ScanSampledWalk {
    active: bool,
    sample_interval: u64,
    sampled_documents: u64,
    path_elapsed: Duration,
    value_hash_elapsed: Duration,
    value_canonicalize_elapsed: Duration,
    field_update_elapsed: Duration,
    sample_update_elapsed: Duration,
}

impl ScanSampledWalk {
    fn new() -> Self {
        Self {
            active: false,
            sample_interval: SCAN_SAMPLED_WALK_INTERVAL,
            sampled_documents: 0,
            path_elapsed: Duration::default(),
            value_hash_elapsed: Duration::default(),
            value_canonicalize_elapsed: Duration::default(),
            field_update_elapsed: Duration::default(),
            sample_update_elapsed: Duration::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct ScanPerfWindow {
    index: u64,
    documents_start: u64,
    objects_start: u64,
    arrays_start: u64,
    scalars_start: u64,
    bytes_since_last: u64,
    parse_elapsed: Duration,
    walk_elapsed: Duration,
    objects_visited: u64,
    arrays_visited: u64,
    scalars_visited: u64,
    field_edges_visited: u64,
    scalar_nulls: u64,
    scalar_booleans: u64,
    scalar_integers: u64,
    scalar_numbers: u64,
    scalar_strings: u64,
    shape_observations: u64,
    field_profile_observations: u64,
    path_strings: u64,
    value_hashes: u64,
    value_canonicalizations: u64,
    field_updates: u64,
    value_observations: u64,
    flush_checks: u64,
    sampled_walk: ScanSampledWalk,
    started_at: Instant,
    walk_started: Option<Instant>,
}

impl ScanPerfWindow {
    fn new(index: u64, counters: SourceCounters) -> Self {
        Self {
            index,
            documents_start: counters.total_document_count,
            objects_start: counters.total_object_count,
            arrays_start: counters.total_array_count,
            scalars_start: counters.total_scalar_count,
            bytes_since_last: 0,
            parse_elapsed: Duration::default(),
            walk_elapsed: Duration::default(),
            objects_visited: 0,
            arrays_visited: 0,
            scalars_visited: 0,
            field_edges_visited: 0,
            scalar_nulls: 0,
            scalar_booleans: 0,
            scalar_integers: 0,
            scalar_numbers: 0,
            scalar_strings: 0,
            shape_observations: 0,
            field_profile_observations: 0,
            path_strings: 0,
            value_hashes: 0,
            value_canonicalizations: 0,
            field_updates: 0,
            value_observations: 0,
            flush_checks: 0,
            sampled_walk: ScanSampledWalk::new(),
            started_at: Instant::now(),
            walk_started: None,
        }
    }
}

struct ProfileRunVisitor {
    config: ProfileConfig,
    resolver: RefsResolver,
    writer: Option<ProfileWriter>,
    counters: SourceCounters,
    scan_perf: ScanPerfWindow,
    shape_accumulator: ShapeAccumulator,
    shape_fields: HashMap<String, ShapeFieldRow>,
    field_values: HashMap<String, FieldValueAccumulator>,
    pending_value_sample_rows: usize,
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
            writer: Some(writer),
            counters: SourceCounters::default(),
            scan_perf: ScanPerfWindow::new(0, SourceCounters::default()),
            shape_accumulator: ShapeAccumulator::default(),
            shape_fields: HashMap::new(),
            field_values: HashMap::new(),
            pending_value_sample_rows: 0,
            warnings,
            warned_canonical_path_unavailable: false,
            perf_log,
        }
    }

    fn record_perf(&mut self, name: &'static str, duration: Duration) {
        self.perf_log.record(name, duration);
    }

    fn perf_enabled(&self) -> bool {
        self.perf_log.enabled()
    }

    fn emit_scan_progress(&mut self) {
        if !self.perf_enabled() {
            return;
        }
        self.perf_log.event(&format!(
            "phase=scan.progress documents={} objects={} arrays={} scalars={}",
            self.counters.total_document_count,
            self.counters.total_object_count,
            self.counters.total_array_count,
            self.counters.total_scalar_count
        ));
    }

    fn emit_scan_accumulators(&mut self) {
        if !self.perf_enabled() {
            return;
        }
        self.perf_log.event(&format!(
            "phase=scan.accumulators index={} pending_shapes={} pending_shape_fields={} pending_object_samples={} pending_value_samples={} field_value_accumulators={}",
            self.scan_perf.index,
            self.shape_accumulator.shape_row_count(),
            self.shape_fields.len(),
            self.shape_accumulator.pending_object_sample_count(),
            self.pending_value_sample_rows,
            self.field_values.len()
        ));
    }

    fn emit_scan_chunk(&mut self, reason: ScanChunkReason) {
        if !self.perf_enabled() {
            return;
        }

        let documents_since_last =
            self.counters.total_document_count - self.scan_perf.documents_start;
        let objects_since_last = self.counters.total_object_count - self.scan_perf.objects_start;
        let arrays_since_last = self.counters.total_array_count - self.scan_perf.arrays_start;
        let scalars_since_last = self.counters.total_scalar_count - self.scan_perf.scalars_start;
        let elapsed_secs = self.scan_perf.started_at.elapsed().as_secs_f64();
        let docs_per_sec = per_second(documents_since_last, elapsed_secs);
        let scalars_per_sec = per_second(scalars_since_last, elapsed_secs);

        self.perf_log.event(&format!(
            "phase=scan.chunk index={} reason={} documents_total={} documents_since_last={} bytes_since_last={} objects_since_last={} arrays_since_last={} scalars_since_last={} parse_ms={} walk_ms={} docs_per_sec={:.1} scalars_per_sec={:.1}",
            self.scan_perf.index,
            reason.as_str(),
            self.counters.total_document_count,
            documents_since_last,
            self.scan_perf.bytes_since_last,
            objects_since_last,
            arrays_since_last,
            scalars_since_last,
            self.scan_perf.parse_elapsed.as_millis(),
            self.scan_perf.walk_elapsed.as_millis(),
            docs_per_sec,
            scalars_per_sec
        ));
    }

    fn emit_scan_hot_counters(&mut self) {
        if !self.perf_enabled() {
            return;
        }
        self.perf_log.event(&format!(
            "phase=scan.hot_counters index={} objects_visited={} arrays_visited={} scalars_visited={} field_edges_visited={} scalar_nulls={} scalar_booleans={} scalar_integers={} scalar_numbers={} scalar_strings={} shape_observations={} field_profile_observations={} value_observations={} flush_checks={} path_strings={} value_hashes={} value_canonicalizations={} field_updates={}",
            self.scan_perf.index,
            self.scan_perf.objects_visited,
            self.scan_perf.arrays_visited,
            self.scan_perf.scalars_visited,
            self.scan_perf.field_edges_visited,
            self.scan_perf.scalar_nulls,
            self.scan_perf.scalar_booleans,
            self.scan_perf.scalar_integers,
            self.scan_perf.scalar_numbers,
            self.scan_perf.scalar_strings,
            self.scan_perf.shape_observations,
            self.scan_perf.field_profile_observations,
            self.scan_perf.value_observations,
            self.scan_perf.flush_checks,
            self.scan_perf.path_strings,
            self.scan_perf.value_hashes,
            self.scan_perf.value_canonicalizations,
            self.scan_perf.field_updates
        ));
    }

    fn emit_scan_sampled_walk(&mut self) {
        if !self.perf_enabled() {
            return;
        }

        let documents_since_last =
            self.counters.total_document_count - self.scan_perf.documents_start;
        let sample_ratio = if documents_since_last > 0 {
            self.scan_perf.sampled_walk.sampled_documents as f64 / documents_since_last as f64
        } else {
            0.0
        };

        self.perf_log.event(&format!(
            "phase=scan.sampled_walk index={} sample_interval={} sampled_documents={} documents_since_last={} sample_ratio={:.6} path_ms={} value_hash_ms={} value_canonicalize_ms={} field_update_ms={} sample_update_ms={}",
            self.scan_perf.index,
            self.scan_perf.sampled_walk.sample_interval,
            self.scan_perf.sampled_walk.sampled_documents,
            documents_since_last,
            sample_ratio,
            self.scan_perf.sampled_walk.path_elapsed.as_millis(),
            self.scan_perf.sampled_walk.value_hash_elapsed.as_millis(),
            self.scan_perf.sampled_walk.value_canonicalize_elapsed.as_millis(),
            self.scan_perf.sampled_walk.field_update_elapsed.as_millis(),
            self.scan_perf.sampled_walk.sample_update_elapsed.as_millis()
        ));
    }

    fn reset_scan_perf_window(&mut self) {
        if !self.perf_enabled() {
            return;
        }
        let next_index = self.scan_perf.index + 1;
        self.scan_perf = ScanPerfWindow::new(next_index, self.counters);
    }

    fn pause_scan_walk_timing(&mut self) -> bool {
        if !self.perf_enabled() {
            return false;
        }
        if let Some(started) = self.scan_perf.walk_started.take() {
            self.scan_perf.walk_elapsed += started.elapsed();
            true
        } else {
            false
        }
    }

    fn resume_scan_walk_timing(&mut self, was_active: bool) {
        if self.perf_enabled() && was_active {
            self.scan_perf.walk_started = Some(Instant::now());
        }
    }

    fn emit_flush_trigger(&mut self, reason: FlushReason) {
        if !self.perf_enabled() {
            return;
        }
        let documents_since_flush =
            self.counters.total_document_count - self.scan_perf.documents_start;
        self.perf_log.event(&format!(
            "phase=flush.trigger index={} reason={} documents_since_flush={} pending_shapes={} pending_shape_fields={} pending_object_samples={} pending_value_samples={} field_value_accumulators={}",
            self.scan_perf.index,
            reason.as_str(),
            documents_since_flush,
            self.shape_accumulator.shape_row_count(),
            self.shape_fields.len(),
            self.shape_accumulator.pending_object_sample_count(),
            self.pending_value_sample_rows,
            self.field_values.len()
        ));
    }

    fn emit_flush_diagnostics(&mut self, reason: FlushReason) -> u64 {
        let index = self.scan_perf.index;
        self.emit_flush_trigger(reason);
        self.emit_scan_chunk(scan_chunk_reason_for_flush(reason));
        self.emit_scan_accumulators();
        self.emit_scan_hot_counters();
        self.emit_scan_sampled_walk();
        self.reset_scan_perf_window();
        index
    }

    fn emit_flush_chunk(&mut self, index: u64, chunk: &ProfileChunk) {
        if !self.perf_enabled() {
            return;
        }
        self.perf_log.event(&format!(
            "phase=flush.chunk index={} shapes={} fields={} object_samples={} field_summaries={} field_values={} value_samples={}",
            index,
            chunk.shapes.len(),
            chunk.shape_fields.len(),
            chunk.object_samples.len(),
            chunk.field_summaries.len(),
            chunk.field_values.len(),
            chunk.value_samples.len()
        ));
    }

    fn emit_dbstat(&mut self) {
        let summaries = self
            .writer
            .as_ref()
            .expect("profile writer is open")
            .dbstat_summaries(8);
        if summaries.is_empty() {
            self.perf_log.event("phase=sqlite.dbstat unavailable=1");
            return;
        }

        for (index, summary) in summaries.iter().enumerate() {
            self.perf_log.event(&format!(
                "phase=sqlite.dbstat rank={} table={} mb={:.3}",
                index + 1,
                summary.top_table,
                summary.mb
            ));
        }
    }

    fn emit_sqlite_size(&mut self, out_path: &std::path::Path) {
        let sqlite_bytes = std::fs::metadata(out_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let wal_bytes = std::fs::metadata(out_path.with_extension("sqlite-wal"))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let shm_bytes = std::fs::metadata(out_path.with_extension("sqlite-shm"))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        self.perf_log.event(&format!(
            "phase=sqlite.size profile_sqlite_bytes={} profile_sqlite_wal_bytes={} profile_sqlite_shm_bytes={}",
            sqlite_bytes, wal_bytes, shm_bytes
        ));
    }

    fn finish(
        mut self,
        source_format: &str,
        out_path: PathBuf,
        quiet: bool,
        elapsed: Duration,
    ) -> Result<ProfileReport> {
        self.flush_pending_samples(FlushReason::FinalSamples)?;

        let final_values_flush_index = self
            .has_pending_field_value_accumulators()
            .then(|| self.emit_flush_diagnostics(FlushReason::FinalFieldValues));
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
        self.flush_chunk(final_chunk, final_values_flush_index)?;
        let index_start = Instant::now();
        self.writer
            .as_ref()
            .expect("profile writer is open")
            .create_indexes()?;
        self.perf_log
            .record("sqlite.indexes", index_start.elapsed());
        self.perf_log
            .elapsed_event("sqlite.indexes", index_start, format_args!("created=1"));
        let summary = self
            .writer
            .as_mut()
            .expect("profile writer is open")
            .write_source_summary(source_format, self.counters, &mut self.perf_log)?;
        self.emit_sqlite_size(&out_path);
        if self.config.perf_log_dbstat {
            self.emit_dbstat();
        }
        self.close_writer()?;
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
        let perf_enabled = self.perf_enabled();
        let sampled_active = perf_enabled && self.scan_perf.sampled_walk.active;

        for (field_name, value) in object {
            if perf_enabled {
                self.scan_perf.field_updates += 1;
                self.scan_perf.field_profile_observations += 1;
                self.scan_perf.path_strings += 1;
                self.scan_perf.value_hashes += 1;
                self.scan_perf.value_observations += 1;
                if value_requires_canonicalization(value) {
                    self.scan_perf.value_canonicalizations += 1;
                }
            }

            let started = sampled_active.then(Instant::now);
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
            if let Some(started) = started {
                self.scan_perf.sampled_walk.field_update_elapsed += started.elapsed();
            }

            let started = sampled_active.then(Instant::now);
            let source_path = field_source_path(object_source_path, field_name);
            if let Some(started) = started {
                self.scan_perf.sampled_walk.path_elapsed += started.elapsed();
            }

            let stats = {
                let accumulator = self
                    .field_values
                    .entry(field_profile_id.clone())
                    .or_insert_with(|| FieldValueAccumulator::new(field_profile_id, &self.config));
                if sampled_active {
                    let mut timing = FieldValueObserveTiming::default();
                    let stats = accumulator.observe_with_timing(
                        document_index,
                        &source_path,
                        value,
                        object,
                        &self.config,
                        &mut timing,
                    );
                    self.scan_perf.sampled_walk.value_hash_elapsed += timing.value_hash_elapsed;
                    self.scan_perf.sampled_walk.value_canonicalize_elapsed +=
                        timing.value_canonicalize_elapsed;
                    self.scan_perf.sampled_walk.field_update_elapsed += timing.field_update_elapsed;
                    self.scan_perf.sampled_walk.sample_update_elapsed +=
                        timing.sample_update_elapsed;
                    stats
                } else {
                    accumulator.observe(document_index, &source_path, value, object, &self.config)
                }
            };
            self.apply_pending_value_sample_delta(stats.pending_value_sample_delta);
        }
    }

    fn flush_if_needed(&mut self) -> Result<()> {
        if self.perf_enabled() {
            self.scan_perf.flush_checks += 1;
        }

        let reason = if self.shape_accumulator.shape_row_count()
            >= self.config.flush.chunk_shape_rows
            || self.shape_accumulator.pending_object_sample_count()
                >= self.config.flush.chunk_object_sample_rows
            || self.shape_fields.len() >= self.config.flush.chunk_field_rows
        {
            Some(FlushReason::DocumentChunkLimit)
        } else if self.pending_value_sample_rows >= self.config.flush.chunk_value_sample_rows {
            Some(FlushReason::ValueSampleLimit)
        } else {
            None
        };

        if let Some(reason) = reason {
            self.flush_pending_samples(reason)?;
        }
        Ok(())
    }

    fn flush_after_object_if_needed(&mut self) -> Result<()> {
        if self.perf_enabled() {
            self.scan_perf.flush_checks += 1;
        }

        let reason = if self.shape_accumulator.shape_row_count()
            >= self.config.flush.chunk_shape_rows
            || self.shape_fields.len() >= self.config.flush.chunk_field_rows
        {
            Some(FlushReason::DocumentChunkLimit)
        } else {
            None
        };

        if let Some(reason) = reason {
            self.flush_pending_samples(reason)?;
        }
        Ok(())
    }

    fn flush_pending_samples(&mut self, reason: FlushReason) -> Result<()> {
        let walk_was_active = self.pause_scan_walk_timing();
        let flush_index = self
            .has_pending_sample_chunk()
            .then(|| self.emit_flush_diagnostics(reason));
        let chunk = self.drain_pending_chunk();
        let result = self.flush_chunk_while_paused(chunk, flush_index);
        self.resume_scan_walk_timing(walk_was_active);
        result
    }

    fn flush_chunk(&mut self, chunk: ProfileChunk, flush_index: Option<u64>) -> Result<()> {
        let walk_was_active = self.pause_scan_walk_timing();
        let result = self.flush_chunk_while_paused(chunk, flush_index);
        self.resume_scan_walk_timing(walk_was_active);
        result
    }

    fn flush_chunk_while_paused(
        &mut self,
        chunk: ProfileChunk,
        flush_index: Option<u64>,
    ) -> Result<()> {
        if !chunk.is_empty() {
            let index =
                flush_index.unwrap_or_else(|| self.emit_flush_diagnostics(FlushReason::Explicit));
            self.emit_flush_chunk(index, &chunk);
        }
        self.writer
            .as_mut()
            .expect("profile writer is open")
            .flush_chunk(chunk, &mut self.perf_log)
    }

    fn close_writer(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.take() {
            writer.close(&mut self.perf_log)?;
        }
        Ok(())
    }

    fn has_pending_sample_chunk(&self) -> bool {
        self.shape_accumulator.shape_row_count() > 0
            || !self.shape_fields.is_empty()
            || self.shape_accumulator.pending_object_sample_count() > 0
            || self.pending_value_sample_rows > 0
    }

    fn has_pending_scan_completion_flush(&self) -> bool {
        self.has_pending_sample_chunk() || self.has_pending_field_value_accumulators()
    }

    fn has_pending_field_value_accumulators(&self) -> bool {
        !self.field_values.is_empty()
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
        let drained = rows.len();
        self.pending_value_sample_rows = self
            .pending_value_sample_rows
            .checked_sub(drained)
            .expect("pending value sample counter underflow during drain");
        #[cfg(debug_assertions)]
        self.debug_assert_pending_value_sample_counter();
        rows
    }

    fn apply_pending_value_sample_delta(&mut self, delta: PendingRowDelta) {
        if delta.added == 0 && delta.removed == 0 {
            return;
        }
        self.pending_value_sample_rows += delta.added;
        self.pending_value_sample_rows = self
            .pending_value_sample_rows
            .checked_sub(delta.removed)
            .expect("pending value sample counter underflow");
        #[cfg(debug_assertions)]
        self.debug_assert_pending_value_sample_counter();
    }

    #[cfg(debug_assertions)]
    fn pending_value_sample_count_slow(&self) -> usize {
        self.field_values
            .values()
            .map(FieldValueAccumulator::pending_value_sample_count)
            .sum()
    }

    #[cfg(debug_assertions)]
    fn debug_assert_pending_value_sample_counter(&self) {
        debug_assert_eq!(
            self.pending_value_sample_rows,
            self.pending_value_sample_count_slow(),
            "pending value sample counter drifted"
        );
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
    fn perf_enabled(&self) -> bool {
        ProfileRunVisitor::perf_enabled(self)
    }

    fn record_scan_bytes(&mut self, bytes: u64) {
        if self.perf_enabled() {
            self.scan_perf.bytes_since_last += bytes;
        }
    }

    fn record_scan_parse_elapsed(&mut self, duration: Duration) {
        if self.perf_enabled() {
            self.scan_perf.parse_elapsed += duration;
        }
    }

    fn begin_scan_walk_timing(&mut self) {
        if self.perf_enabled() {
            self.scan_perf.walk_started = Some(Instant::now());
        }
    }

    fn end_scan_walk_timing(&mut self) {
        if !self.perf_enabled() {
            return;
        }
        if let Some(started) = self.scan_perf.walk_started.take() {
            self.scan_perf.walk_elapsed += started.elapsed();
        }
    }

    fn begin_document(&mut self, _document_index: u64) -> Result<()> {
        self.counters.total_document_count += 1;
        if self.perf_enabled() {
            let document_ordinal = self.counters.total_document_count - 1;
            #[allow(clippy::manual_is_multiple_of)]
            let active = document_ordinal % self.scan_perf.sampled_walk.sample_interval == 0;
            self.scan_perf.sampled_walk.active = active;
            if active {
                self.scan_perf.sampled_walk.sampled_documents += 1;
            }
        }
        Ok(())
    }

    fn end_document(&mut self, _document_index: u64) -> Result<()> {
        let result = self.flush_if_needed();
        if self.perf_enabled() {
            self.scan_perf.sampled_walk.active = false;
        }
        result
    }

    fn visit_object(
        &mut self,
        document_index: u64,
        path: &SourcePath,
        object: &Map<String, Value>,
    ) -> Result<()> {
        self.counters.total_object_count += 1;
        let perf_enabled = self.perf_enabled();
        if perf_enabled {
            self.scan_perf.objects_visited += 1;
            self.scan_perf.field_edges_visited += object.len() as u64;
            self.scan_perf.shape_observations += 1;
            self.scan_perf.path_strings += 2;
        }

        let context = self.resolver.resolve_object(path);
        if !context.resolved {
            self.warn_unresolved_context_once(path);
        }
        let object_source_path = path.as_str();
        let facts = self.shape_accumulator.observe_object_with_facts(
            document_index,
            path,
            &context,
            object,
            &self.config.sampling,
        )?;
        self.observe_object_fields(document_index, &object_source_path, &facts.shape_id, object);
        self.flush_after_object_if_needed()
    }

    fn visit_array(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        _array: &[Value],
    ) -> Result<()> {
        self.counters.total_array_count += 1;
        if self.perf_enabled() {
            self.scan_perf.arrays_visited += 1;
        }
        Ok(())
    }

    fn visit_scalar(
        &mut self,
        _document_index: u64,
        _path: &SourcePath,
        value: &Value,
    ) -> Result<()> {
        self.counters.total_scalar_count += 1;
        if self.perf_enabled() {
            self.scan_perf.scalars_visited += 1;
            match value {
                Value::Null => self.scan_perf.scalar_nulls += 1,
                Value::Bool(_) => self.scan_perf.scalar_booleans += 1,
                Value::Number(number) if number.is_i64() || number.is_u64() => {
                    self.scan_perf.scalar_integers += 1;
                }
                Value::Number(_) => self.scan_perf.scalar_numbers += 1,
                Value::String(_) => self.scan_perf.scalar_strings += 1,
                Value::Array(_) | Value::Object(_) => {}
            }
        }
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

fn per_second(count: u64, elapsed_secs: f64) -> f64 {
    if elapsed_secs > 0.0 {
        count as f64 / elapsed_secs
    } else {
        0.0
    }
}

fn scan_chunk_reason_for_flush(reason: FlushReason) -> ScanChunkReason {
    match reason {
        FlushReason::FinalSamples | FlushReason::FinalFieldValues => ScanChunkReason::Final,
        FlushReason::ValueSampleLimit | FlushReason::DocumentChunkLimit | FlushReason::Explicit => {
            ScanChunkReason::PreFlush
        }
    }
}

fn value_requires_canonicalization(value: &Value) -> bool {
    matches!(value, Value::Array(_) | Value::Object(_))
}
