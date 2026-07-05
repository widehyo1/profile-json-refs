use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use rusqlite::{Connection, Transaction, params};

use crate::SourceSummary;
use crate::config::ProfileConfig;
use crate::error::Result;
use crate::field::accumulator::ShapeFieldRow;
use crate::field::summary::FieldSummary;
use crate::perf::timer::PerfLog;
use crate::shape::accumulator::ShapeRow;
use crate::shape::sample::{ObjectSampleKind, ObjectSampleRow, SampleScope};
use crate::value::exact_counter::FieldValueRow;
use crate::value::sample::{ValueSampleKind, ValueSampleRow};

#[derive(Debug, Default)]
pub struct ProfileChunk {
    pub shapes: Vec<ShapeRow>,
    pub shape_fields: Vec<ShapeFieldRow>,
    pub object_samples: Vec<ObjectSampleRow>,
    pub field_summaries: Vec<FieldSummary>,
    pub field_values: Vec<FieldValueRow>,
    pub value_samples: Vec<ValueSampleRow>,
}

#[derive(Debug, Default)]
struct TouchedSampleKeys {
    object_priority: BTreeSet<(SampleScope, String)>,
    value_priority_fields: BTreeSet<String>,
    heavy_hitter_context: BTreeSet<(String, String)>,
}

impl TouchedSampleKeys {
    fn from_chunk(chunk: &ProfileChunk) -> Self {
        let object_priority = chunk
            .object_samples
            .iter()
            .filter(|row| row.sample_kind == ObjectSampleKind::PrioritySample)
            .map(|row| (row.sample_scope, row.sample_key.clone()))
            .collect();
        let value_priority_fields = chunk
            .value_samples
            .iter()
            .filter(|row| row.sample_kind == ValueSampleKind::PrioritySample)
            .map(|row| row.field_profile_id.clone())
            .collect();
        let heavy_hitter_context = chunk
            .value_samples
            .iter()
            .filter(|row| row.sample_kind == ValueSampleKind::HeavyHitterContext)
            .filter_map(|row| {
                row.value_hash
                    .as_ref()
                    .map(|hash| (row.field_profile_id.clone(), hash.clone()))
            })
            .collect();

        Self {
            object_priority,
            value_priority_fields,
            heavy_hitter_context,
        }
    }
}

impl ProfileChunk {
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
            && self.shape_fields.is_empty()
            && self.object_samples.is_empty()
            && self.field_summaries.is_empty()
            && self.field_values.is_empty()
            && self.value_samples.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceCounters {
    pub total_document_count: u64,
    pub total_object_count: u64,
    pub total_array_count: u64,
    pub total_scalar_count: u64,
}

pub struct ProfileWriter {
    conn: Connection,
    object_sample_priority_limits: ObjectSamplePriorityLimits,
    value_priority_limit: usize,
    heavy_hitter_context_sample_limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbStatSummary {
    pub top_table: String,
    pub mb: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectSamplePriorityLimits {
    pub canonical_path: usize,
    pub site_path: usize,
    pub field_set: usize,
    pub type_set: usize,
}

impl ObjectSamplePriorityLimits {
    fn limit_for(self, scope: SampleScope) -> usize {
        match scope {
            SampleScope::CanonicalPath => self.canonical_path,
            SampleScope::SitePath => self.site_path,
            SampleScope::FieldSet => self.field_set,
            SampleScope::TypeSet => self.type_set,
        }
    }
}

impl ProfileWriter {
    pub fn open(path: &Path, config: &ProfileConfig) -> Result<Self> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        crate::sqlite::schema::configure_connection(&conn)?;
        crate::sqlite::schema::create_schema(&conn)?;
        crate::sqlite::schema::create_indexes(&conn)?;

        Ok(Self {
            conn,
            object_sample_priority_limits: ObjectSamplePriorityLimits {
                canonical_path: config.sampling.canonical_priority_limit,
                site_path: config.sampling.site_priority_limit,
                field_set: config.sampling.field_set_priority_limit,
                type_set: config.sampling.type_set_priority_limit,
            },
            value_priority_limit: config.sampling.value_priority_limit_per_field_profile,
            heavy_hitter_context_sample_limit: config.sampling.heavy_hitter_context_sample_limit,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn flush_chunk(&mut self, chunk: ProfileChunk, perf_log: &mut PerfLog) -> Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }

        let touched_samples = TouchedSampleKeys::from_chunk(&chunk);
        let tx = self.conn.transaction()?;

        let started = Instant::now();
        Self::write_shapes(&tx, &chunk.shapes)?;
        perf_log.elapsed_event(
            "sqlite.flush.shapes",
            started,
            format_args!("rows={}", chunk.shapes.len()),
        );

        let started = Instant::now();
        Self::write_shape_fields(&tx, &chunk.shape_fields)?;
        perf_log.elapsed_event(
            "sqlite.flush.shape_fields",
            started,
            format_args!("rows={}", chunk.shape_fields.len()),
        );

        let started = Instant::now();
        Self::write_object_samples(&tx, &chunk.object_samples)?;
        perf_log.elapsed_event(
            "sqlite.flush.object_samples",
            started,
            format_args!("rows={}", chunk.object_samples.len()),
        );

        let started = Instant::now();
        Self::write_field_summaries(&tx, &chunk.field_summaries)?;
        perf_log.elapsed_event(
            "sqlite.flush.field_summaries",
            started,
            format_args!("rows={}", chunk.field_summaries.len()),
        );

        let started = Instant::now();
        Self::write_field_values(&tx, &chunk.field_values)?;
        perf_log.elapsed_event(
            "sqlite.flush.field_values",
            started,
            format_args!("rows={}", chunk.field_values.len()),
        );

        let started = Instant::now();
        Self::write_value_samples(&tx, &chunk.value_samples)?;
        perf_log.elapsed_event(
            "sqlite.flush.value_samples",
            started,
            format_args!("rows={}", chunk.value_samples.len()),
        );

        let started = Instant::now();
        tx.commit()?;
        perf_log.elapsed_event("sqlite.flush.commit", started, format_args!("rows=0"));

        self.prune_object_priority_samples(&touched_samples.object_priority)?;
        self.prune_value_priority_samples(&touched_samples.value_priority_fields)?;
        if self.heavy_hitter_context_sample_limit > 0 {
            self.prune_heavy_hitter_context_samples(&touched_samples.heavy_hitter_context)?;
        }

        Ok(())
    }

    pub fn create_indexes(&self) -> Result<()> {
        crate::sqlite::schema::create_indexes(&self.conn)?;
        Ok(())
    }

    pub fn dbstat_summary(&self) -> Option<DbStatSummary> {
        let mut stmt = self
            .conn
            .prepare(
                "\
                SELECT name, SUM(pgsize) AS bytes
                FROM dbstat
                GROUP BY name
                ORDER BY bytes DESC
                LIMIT 1
                ",
            )
            .ok()?;

        stmt.query_row([], |row| {
            let top_table: String = row.get(0)?;
            let bytes: i64 = row.get(1)?;
            Ok(DbStatSummary {
                top_table,
                mb: bytes as f64 / (1024.0 * 1024.0),
            })
        })
        .ok()
    }

    pub fn write_source_summary(
        &mut self,
        source_format: &str,
        counters: SourceCounters,
    ) -> Result<SourceSummary> {
        let total_canonical_path_count = self.query_count(
            "SELECT COUNT(DISTINCT canonical_path) FROM prof_shape",
            "canonical path count",
        )?;
        let total_site_path_count = self.query_count(
            "SELECT COUNT(DISTINCT COALESCE(site_path, '')) FROM prof_shape",
            "site path count",
        )?;
        let total_shape_count =
            self.query_count("SELECT COUNT(*) FROM prof_shape", "shape count")?;
        let total_field_profile_count = self.query_count(
            "SELECT COUNT(*) FROM prof_shape_field",
            "field profile count",
        )?;
        let total_stored_value_count = self.query_count(
            "SELECT COUNT(*) FROM prof_field_value",
            "stored value count",
        )?;

        let summary = SourceSummary {
            total_document_count: counters.total_document_count,
            total_object_count: counters.total_object_count,
            total_array_count: counters.total_array_count,
            total_scalar_count: counters.total_scalar_count,
            total_canonical_path_count,
            total_site_path_count,
            total_shape_count,
            total_field_profile_count,
            total_stored_value_count,
        };

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM prof_source_summary", [])?;
        tx.execute(
            "\
            INSERT INTO prof_source_summary (
                source_format,
                total_document_count,
                total_object_count,
                total_array_count,
                total_scalar_count,
                total_canonical_path_count,
                total_site_path_count,
                total_shape_count,
                total_field_profile_count,
                total_stored_value_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ",
            params![
                source_format,
                as_i64(summary.total_document_count),
                as_i64(summary.total_object_count),
                as_i64(summary.total_array_count),
                as_i64(summary.total_scalar_count),
                as_i64(summary.total_canonical_path_count),
                as_i64(summary.total_site_path_count),
                as_i64(summary.total_shape_count),
                as_i64(summary.total_field_profile_count),
                as_i64(summary.total_stored_value_count),
            ],
        )?;
        tx.commit()?;

        Ok(summary)
    }

    fn query_count(&self, sql: &str, _label: &str) -> Result<u64> {
        Ok(self.conn.query_row(sql, [], |row| row.get::<_, u64>(0))?)
    }

    fn write_shapes(tx: &Transaction<'_>, rows: &[ShapeRow]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT INTO prof_shape (
                shape_id,
                canonical_path,
                site_path,
                schema_path,
                field_set_hash,
                type_set_hash,
                field_set_json,
                type_set_json,
                object_count,
                first_seen_document_index,
                first_seen_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(shape_id) DO UPDATE SET
                object_count = object_count + excluded.object_count
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.shape_id,
                row.canonical_path,
                row.site_path,
                row.schema_path,
                row.field_set_hash,
                row.type_set_hash,
                row.field_set_json,
                row.type_set_json,
                as_i64(row.object_count),
                row.first_seen_document_index.map(as_i64),
                row.first_seen_path,
            ])?;
        }
        Ok(())
    }

    fn write_shape_fields(tx: &Transaction<'_>, rows: &[ShapeFieldRow]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT INTO prof_shape_field (
                field_profile_id,
                shape_id,
                field_name,
                observed_type,
                observed_count,
                null_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(field_profile_id) DO UPDATE SET
                observed_count = observed_count + excluded.observed_count,
                null_count = null_count + excluded.null_count
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.field_profile_id,
                row.shape_id,
                row.field_name,
                row.observed_type.as_sql_str(),
                as_i64(row.observed_count),
                as_i64(row.null_count),
            ])?;
        }
        Ok(())
    }

    fn write_object_samples(tx: &Transaction<'_>, rows: &[ObjectSampleRow]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT OR IGNORE INTO prof_object_sample (
                object_sample_id,
                sample_scope,
                sample_key,
                canonical_path,
                site_path,
                schema_path,
                field_set_hash,
                type_set_hash,
                shape_id,
                sample_kind,
                document_index,
                source_path,
                sample_json,
                sample_json_truncated,
                sample_is_empty_object,
                sample_is_empty_array,
                sample_priority,
                sample_rank
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.object_sample_id,
                row.sample_scope.as_sql_str(),
                row.sample_key,
                row.canonical_path,
                row.site_path,
                row.schema_path,
                row.field_set_hash,
                row.type_set_hash,
                row.shape_id,
                row.sample_kind.as_sql_str(),
                as_i64(row.document_index),
                row.source_path,
                row.sample_json,
                bool_i64(row.sample_json_truncated),
                bool_i64(row.sample_is_empty_object),
                bool_i64(row.sample_is_empty_array),
                row.sample_priority.map(as_i64),
                row.sample_rank.map(|value| value as i64),
            ])?;
        }
        Ok(())
    }

    fn write_field_summaries(tx: &Transaction<'_>, rows: &[FieldSummary]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT INTO prof_field_summary (
                field_profile_id,
                profiled_count,
                null_count,
                non_null_count,
                empty_object_count,
                empty_array_count,
                empty_string_count,
                distinct_count,
                distinct_count_method,
                distinct_algorithm,
                distinct_error_rate,
                stored_value_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(field_profile_id) DO UPDATE SET
                profiled_count = excluded.profiled_count,
                null_count = excluded.null_count,
                non_null_count = excluded.non_null_count,
                empty_object_count = excluded.empty_object_count,
                empty_array_count = excluded.empty_array_count,
                empty_string_count = excluded.empty_string_count,
                distinct_count = excluded.distinct_count,
                distinct_count_method = excluded.distinct_count_method,
                distinct_algorithm = excluded.distinct_algorithm,
                distinct_error_rate = excluded.distinct_error_rate,
                stored_value_count = excluded.stored_value_count
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.field_profile_id,
                as_i64(row.profiled_count),
                as_i64(row.null_count),
                as_i64(row.non_null_count),
                as_i64(row.empty_object_count),
                as_i64(row.empty_array_count),
                as_i64(row.empty_string_count),
                row.distinct_count.map(as_i64),
                row.distinct_count_method.as_sql_str(),
                row.distinct_algorithm
                    .map(|algorithm| algorithm.as_sql_str()),
                row.distinct_error_rate,
                as_i64(row.stored_value_count),
            ])?;
        }
        Ok(())
    }

    fn write_field_values(tx: &Transaction<'_>, rows: &[FieldValueRow]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT INTO prof_field_value (
                field_profile_id,
                value_hash,
                value_type,
                value_text,
                value_text_truncated,
                count,
                count_method,
                value_source,
                rank,
                is_complete_distribution
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(field_profile_id, value_hash, value_source) DO UPDATE SET
                value_type = excluded.value_type,
                value_text = excluded.value_text,
                value_text_truncated = excluded.value_text_truncated,
                count = excluded.count,
                count_method = excluded.count_method,
                rank = excluded.rank,
                is_complete_distribution = excluded.is_complete_distribution
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.field_profile_id,
                row.value_hash,
                row.value_type.as_sql_str(),
                row.value_text,
                bool_i64(row.value_text_truncated),
                row.count.map(as_i64),
                row.count_method.as_sql_str(),
                row.value_source.as_sql_str(),
                row.rank.map(|value| value as i64),
                bool_i64(row.is_complete_distribution),
            ])?;
        }
        Ok(())
    }

    fn write_value_samples(tx: &Transaction<'_>, rows: &[ValueSampleRow]) -> Result<()> {
        let mut stmt = tx.prepare(
            "\
            INSERT OR IGNORE INTO prof_field_value_sample (
                value_sample_id,
                field_profile_id,
                value_hash,
                sample_kind,
                document_index,
                source_path,
                value_json,
                value_json_truncated,
                parent_object_json,
                parent_object_json_truncated,
                sample_priority,
                sample_rank
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ",
        )?;

        for row in rows {
            stmt.execute(params![
                row.value_sample_id,
                row.field_profile_id,
                row.value_hash,
                row.sample_kind.as_sql_str(),
                as_i64(row.document_index),
                row.source_path,
                row.value_json,
                bool_i64(row.value_json_truncated),
                row.parent_object_json,
                bool_i64(row.parent_object_json_truncated),
                row.sample_priority.map(as_i64),
                row.sample_rank.map(|value| value as i64),
            ])?;
        }
        Ok(())
    }

    fn prune_object_priority_samples(
        &self,
        touched_keys: &BTreeSet<(SampleScope, String)>,
    ) -> Result<()> {
        for (scope, key) in touched_keys {
            let scope_sql = scope.as_sql_str();
            let limit = self.object_sample_priority_limits.limit_for(*scope);
            self.conn.execute(
                "\
                DELETE FROM prof_object_sample
                WHERE sample_kind = 'priority_sample'
                  AND sample_scope = ?1
                  AND sample_key = ?2
                  AND object_sample_id IN (
                    SELECT object_sample_id
                    FROM (
                      SELECT
                        object_sample_id,
                        ROW_NUMBER() OVER (
                          PARTITION BY sample_scope, sample_key
                          ORDER BY sample_priority ASC, document_index ASC, source_path ASC
                        ) AS rn
                      FROM prof_object_sample
                      WHERE sample_kind = 'priority_sample'
                        AND sample_scope = ?1
                        AND sample_key = ?2
                    )
                    WHERE rn > ?3
                  )
                ",
                params![scope_sql, key, limit as i64],
            )?;

            self.conn.execute(
                "\
                WITH ranked AS (
                  SELECT
                    object_sample_id,
                    ROW_NUMBER() OVER (
                      PARTITION BY sample_scope, sample_key
                      ORDER BY sample_priority ASC, document_index ASC, source_path ASC
                    ) AS rn
                  FROM prof_object_sample
                  WHERE sample_kind = 'priority_sample'
                    AND sample_scope = ?1
                    AND sample_key = ?2
                )
                UPDATE prof_object_sample
                SET sample_rank = (
                  SELECT rn FROM ranked WHERE ranked.object_sample_id = prof_object_sample.object_sample_id
                )
                WHERE object_sample_id IN (SELECT object_sample_id FROM ranked)
                ",
                params![scope_sql, key],
            )?;
        }
        Ok(())
    }

    fn prune_value_priority_samples(&self, touched_fields: &BTreeSet<String>) -> Result<()> {
        for field_profile_id in touched_fields {
            self.conn.execute(
                "\
                DELETE FROM prof_field_value_sample
                WHERE sample_kind = 'priority_sample'
                  AND field_profile_id = ?1
                  AND value_sample_id IN (
                    SELECT value_sample_id
                    FROM (
                      SELECT
                        value_sample_id,
                        ROW_NUMBER() OVER (
                          PARTITION BY field_profile_id
                          ORDER BY sample_priority ASC, document_index ASC, source_path ASC
                        ) AS rn
                      FROM prof_field_value_sample
                      WHERE sample_kind = 'priority_sample'
                        AND field_profile_id = ?1
                    )
                    WHERE rn > ?2
                  )
                ",
                params![field_profile_id, self.value_priority_limit as i64],
            )?;

            self.conn.execute(
                "\
                WITH ranked AS (
                  SELECT
                    value_sample_id,
                    ROW_NUMBER() OVER (
                      PARTITION BY field_profile_id
                      ORDER BY sample_priority ASC, document_index ASC, source_path ASC
                    ) AS rn
                  FROM prof_field_value_sample
                  WHERE sample_kind = 'priority_sample'
                    AND field_profile_id = ?1
                )
                UPDATE prof_field_value_sample
                SET sample_rank = (
                  SELECT rn FROM ranked WHERE ranked.value_sample_id = prof_field_value_sample.value_sample_id
                )
                WHERE value_sample_id IN (SELECT value_sample_id FROM ranked)
                ",
                [field_profile_id],
            )?;
        }
        Ok(())
    }

    fn prune_heavy_hitter_context_samples(
        &self,
        touched_keys: &BTreeSet<(String, String)>,
    ) -> Result<()> {
        for (field_profile_id, value_hash) in touched_keys {
            self.conn.execute(
                "\
                DELETE FROM prof_field_value_sample
                WHERE sample_kind = 'heavy_hitter_context'
                  AND field_profile_id = ?1
                  AND value_hash = ?2
                  AND value_sample_id IN (
                    SELECT value_sample_id
                    FROM (
                      SELECT
                        value_sample_id,
                        ROW_NUMBER() OVER (
                          PARTITION BY field_profile_id, value_hash
                          ORDER BY document_index ASC, source_path ASC
                        ) AS rn
                      FROM prof_field_value_sample
                      WHERE sample_kind = 'heavy_hitter_context'
                        AND field_profile_id = ?1
                        AND value_hash = ?2
                    )
                    WHERE rn > ?3
                  )
                ",
                params![
                    field_profile_id,
                    value_hash,
                    self.heavy_hitter_context_sample_limit as i64
                ],
            )?;
        }
        Ok(())
    }
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).expect("profile counters fit in SQLite INTEGER")
}

fn bool_i64(value: bool) -> i64 {
    i64::from(value)
}
