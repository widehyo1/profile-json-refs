#!/usr/bin/env bash
# Diagnose a profile-json-refs profile.sqlite artifact.
#
# This script is read-only. It reports table sizes, row counts, sample distribution,
# heavy_hitter_context explosion risks, parent_object_json pressure, and v0.1.x
# contract risks.
#
# Usage:
#   ./diagnose_profile_sqlite.sh profile.sqlite
#
# CI-style risk failure:
#   ./diagnose_profile_sqlite.sh --fail-on-risk profile.sqlite
#
# Tunable expected limits:
#   ./diagnose_profile_sqlite.sh \
#     --heavy-hitter-limit 128 \
#     --hh-context-limit 0 \
#     --value-sample-limit 4 \
#     profile.sqlite

set -euo pipefail

DB=""
FAIL_ON_RISK=0
TOP_N=20

HEAVY_HITTER_LIMIT=128
HH_CONTEXT_LIMIT=0
VALUE_SAMPLE_LIMIT=4

OBJECT_CANONICAL_PRIORITY_LIMIT=1
OBJECT_SITE_PRIORITY_LIMIT=1
OBJECT_FIELD_SET_PRIORITY_LIMIT=2
OBJECT_TYPE_SET_PRIORITY_LIMIT=4

usage() {
  cat <<'EOF'
Usage:
  diagnose_profile_sqlite.sh [OPTIONS] <profile.sqlite>

Options:
  --fail-on-risk
      Exit non-zero when contract/performance risks are detected.

  --top <N>
      Number of top rows to show for diagnostic rankings. Default: 20.

  --heavy-hitter-limit <N>
      Expected heavy hitter candidate limit per field_profile_id. Default: 128.

  --hh-context-limit <N>
      Expected heavy_hitter_context samples per final heavy hitter value.
      v0.1.0-rc.2 performance-safe default is 0. Default: 0.

  --value-sample-limit <N>
      Expected priority_sample limit per field_profile_id. Default: 4.

  --object-canonical-limit <N>
      Expected object priority_sample limit for canonical_path. Default: 1.

  --object-site-limit <N>
      Expected object priority_sample limit for site_path. Default: 1.

  --object-field-set-limit <N>
      Expected object priority_sample limit for field_set. Default: 2.

  --object-type-set-limit <N>
      Expected object priority_sample limit for type_set. Default: 4.

  -h, --help
      Show this help.
EOF
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

section() {
  printf '\n============================================================\n'
  printf '%s\n' "$1"
  printf '============================================================\n'
}

subsection() {
  printf '\n-- %s --\n' "$1"
}

sql() {
  sqlite3 -readonly "$DB" "$@"
}

sql_scalar() {
  local query="$1"
  sqlite3 -readonly "$DB" "$query" 2>/dev/null || true
}

table_exists() {
  local table="$1"
  local n
  n="$(sql_scalar "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='$table';")"
  [[ "${n:-0}" = "1" ]]
}

column_exists() {
  local table="$1"
  local column="$2"
  local n
  n="$(sqlite3 -readonly "$DB" "SELECT COUNT(*) FROM pragma_table_info('$table') WHERE name='$column';" 2>/dev/null || true)"
  [[ "${n:-0}" = "1" ]]
}

risk_count=0

risk() {
  risk_count=$((risk_count + 1))
  echo "RISK: $*" >&2
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --fail-on-risk)
        FAIL_ON_RISK=1
        shift
        ;;
      --top)
        TOP_N="${2:?--top requires a value}"
        shift 2
        ;;
      --heavy-hitter-limit)
        HEAVY_HITTER_LIMIT="${2:?--heavy-hitter-limit requires a value}"
        shift 2
        ;;
      --hh-context-limit)
        HH_CONTEXT_LIMIT="${2:?--hh-context-limit requires a value}"
        shift 2
        ;;
      --value-sample-limit)
        VALUE_SAMPLE_LIMIT="${2:?--value-sample-limit requires a value}"
        shift 2
        ;;
      --object-canonical-limit)
        OBJECT_CANONICAL_PRIORITY_LIMIT="${2:?--object-canonical-limit requires a value}"
        shift 2
        ;;
      --object-site-limit)
        OBJECT_SITE_PRIORITY_LIMIT="${2:?--object-site-limit requires a value}"
        shift 2
        ;;
      --object-field-set-limit)
        OBJECT_FIELD_SET_PRIORITY_LIMIT="${2:?--object-field-set-limit requires a value}"
        shift 2
        ;;
      --object-type-set-limit)
        OBJECT_TYPE_SET_PRIORITY_LIMIT="${2:?--object-type-set-limit requires a value}"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      --)
        shift
        break
        ;;
      -*)
        die "unknown option: $1"
        ;;
      *)
        if [[ -n "$DB" ]]; then
          die "multiple database paths given: $DB and $1"
        fi
        DB="$1"
        shift
        ;;
    esac
  done

  [[ -n "$DB" ]] || die "missing profile.sqlite path"
  [[ -f "$DB" ]] || die "not a file: $DB"
  command -v sqlite3 >/dev/null 2>&1 || die "sqlite3 is required"
}

print_file_sizes() {
  section "File sizes"

  local wal="${DB}-wal"
  local shm="${DB}-shm"

  ls -lh "$DB" 2>/dev/null || true
  [[ -e "$wal" ]] && ls -lh "$wal" || true
  [[ -e "$shm" ]] && ls -lh "$shm" || true

  if command -v du >/dev/null 2>&1; then
    du -h "$DB" 2>/dev/null || true
    [[ -e "$wal" ]] && du -h "$wal" || true
    [[ -e "$shm" ]] && du -h "$shm" || true
  fi
}

print_schema_contract() {
  section "Schema contract"

  subsection "prof_* tables"
  sql <<'SQL'
.headers on
.mode column
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND name LIKE 'prof_%'
ORDER BY name;
SQL

  subsection "views"
  local view_count
  view_count="$(sql_scalar "SELECT COUNT(*) FROM sqlite_master WHERE type='view';")"
  echo "view_count=${view_count:-unknown}"
  if [[ "${view_count:-0}" != "0" ]]; then
    risk "SQLite views exist; v0.1.x contract expects no views"
    sql ".headers on" ".mode column" "SELECT name FROM sqlite_master WHERE type='view' ORDER BY name;"
  fi

  subsection "forbidden tables"
  local forbidden_count
  forbidden_count="$(sql_scalar "
SELECT COUNT(*)
FROM sqlite_master
WHERE type = 'table'
  AND (
    name IN (
      'prof_path_sample',
      'prof_shape_sample',
      'prof_run',
      'prof_manifest',
      'prof_algorithm',
      'prof_warning'
    )
    OR name LIKE 'prof_array_%'
  );")"
  echo "forbidden_table_count=${forbidden_count:-unknown}"
  if [[ "${forbidden_count:-0}" != "0" ]]; then
    risk "forbidden tables exist"
    sql <<'SQL'
.headers on
.mode column
SELECT name
FROM sqlite_master
WHERE type = 'table'
  AND (
    name IN (
      'prof_path_sample',
      'prof_shape_sample',
      'prof_run',
      'prof_manifest',
      'prof_algorithm',
      'prof_warning'
    )
    OR name LIKE 'prof_array_%'
  )
ORDER BY name;
SQL
  fi

  subsection "required table presence"
  local required=(
    prof_source_summary
    prof_object_sample
    prof_shape
    prof_shape_field
    prof_field_summary
    prof_field_value
    prof_field_value_sample
  )
  for t in "${required[@]}"; do
    if table_exists "$t"; then
      echo "ok table=$t"
    else
      echo "missing table=$t"
      risk "missing required table: $t"
    fi
  done

  if table_exists prof_field_summary; then
    if column_exists prof_field_summary empty_string_count; then
      echo "ok column=prof_field_summary.empty_string_count"
    else
      echo "missing column=prof_field_summary.empty_string_count"
      risk "missing empty_string_count; current docs treat \"\" as string fact"
    fi
  fi
}

print_row_counts() {
  section "Row counts"

  sql <<'SQL'
.headers on
.mode column
SELECT 'prof_shape' AS table_name, COUNT(*) AS rows FROM prof_shape
UNION ALL SELECT 'prof_shape_field', COUNT(*) FROM prof_shape_field
UNION ALL SELECT 'prof_object_sample', COUNT(*) FROM prof_object_sample
UNION ALL SELECT 'prof_field_summary', COUNT(*) FROM prof_field_summary
UNION ALL SELECT 'prof_field_value', COUNT(*) FROM prof_field_value
UNION ALL SELECT 'prof_field_value_sample', COUNT(*) FROM prof_field_value_sample;
SQL

  local summary_rows field_summary_rows field_value_rows
  summary_rows="$(sql_scalar "SELECT COUNT(*) FROM prof_source_summary;" || true)"
  field_summary_rows="$(sql_scalar "SELECT COUNT(*) FROM prof_field_summary;" || true)"
  field_value_rows="$(sql_scalar "SELECT COUNT(*) FROM prof_field_value;" || true)"

  echo "prof_source_summary rows=${summary_rows:-unknown}"

  if [[ "${summary_rows:-0}" = "0" ]]; then
    risk "prof_source_summary is empty; run may not have finalized"
  fi
  if [[ "${field_summary_rows:-0}" = "0" ]]; then
    risk "prof_field_summary is empty; value finalization may not have completed"
  fi
  if [[ "${field_value_rows:-0}" = "0" ]]; then
    risk "prof_field_value is empty; value distribution finalization may not have completed"
  fi
}

print_dbstat() {
  section "SQLite object sizes"

  if sqlite3 -readonly "$DB" "SELECT COUNT(*) FROM dbstat;" >/dev/null 2>&1; then
    sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT
  name,
  ROUND(SUM(pgsize) / 1024.0 / 1024.0, 2) AS mb
FROM dbstat
GROUP BY name
ORDER BY SUM(pgsize) DESC
LIMIT $TOP_N;
SQL
  else
    echo "dbstat is not available in this sqlite3 build"
  fi
}

print_samples() {
  section "Sample distribution"

  if table_exists prof_object_sample; then
    subsection "object sample kinds"
    sql <<'SQL'
.headers on
.mode column
SELECT sample_scope, sample_kind, COUNT(*) AS rows
FROM prof_object_sample
GROUP BY sample_scope, sample_kind
ORDER BY sample_scope, rows DESC;
SQL
  fi

  if table_exists prof_field_value_sample; then
    subsection "field value sample kinds"
    sql <<'SQL'
.headers on
.mode column
SELECT sample_kind, COUNT(*) AS rows
FROM prof_field_value_sample
GROUP BY sample_kind
ORDER BY rows DESC;
SQL

    subsection "field value sample payload sizes"
    sql <<'SQL'
.headers on
.mode column
SELECT
  sample_kind,
  COUNT(*) AS rows,
  ROUND(SUM(LENGTH(COALESCE(value_json, ''))) / 1024.0 / 1024.0, 2) AS value_json_mb,
  ROUND(SUM(LENGTH(COALESCE(parent_object_json, ''))) / 1024.0 / 1024.0, 2) AS parent_json_mb,
  ROUND(AVG(LENGTH(COALESCE(parent_object_json, ''))), 1) AS avg_parent_json,
  MAX(LENGTH(COALESCE(parent_object_json, ''))) AS max_parent_json
FROM prof_field_value_sample
GROUP BY sample_kind
ORDER BY parent_json_mb DESC;
SQL
  fi
}

print_value_distribution() {
  section "Value distribution"

  if table_exists prof_field_value; then
    subsection "value_source counts"
    sql <<'SQL'
.headers on
.mode column
SELECT value_source, count_method, is_complete_distribution, COUNT(*) AS rows
FROM prof_field_value
GROUP BY value_source, count_method, is_complete_distribution
ORDER BY rows DESC;
SQL
  fi

  if table_exists prof_field_summary; then
    subsection "distinct count methods"
    sql <<'SQL'
.headers on
.mode column
SELECT distinct_count_method, distinct_algorithm, COUNT(*) AS rows
FROM prof_field_summary
GROUP BY distinct_count_method, distinct_algorithm
ORDER BY rows DESC;
SQL
  fi
}

check_heavy_hitter_context() {
  section "Heavy hitter context diagnostics"

  if ! table_exists prof_field_value_sample; then
    echo "prof_field_value_sample not found"
    return
  fi

  local hh_rows
  hh_rows="$(sql_scalar "SELECT COUNT(*) FROM prof_field_value_sample WHERE sample_kind='heavy_hitter_context';")"
  echo "heavy_hitter_context_rows=${hh_rows:-unknown}"
  echo "expected_default_hh_context_limit=$HH_CONTEXT_LIMIT"

  if [[ "$HH_CONTEXT_LIMIT" = "0" && "${hh_rows:-0}" != "0" ]]; then
    risk "heavy_hitter_context rows exist while expected limit is 0"
  fi

  subsection "top field profiles by heavy_hitter_context rows"
  sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT
  field_profile_id,
  COUNT(*) AS rows,
  COUNT(DISTINCT value_hash) AS distinct_values
FROM prof_field_value_sample
WHERE sample_kind = 'heavy_hitter_context'
GROUP BY field_profile_id
ORDER BY rows DESC
LIMIT $TOP_N;
SQL

  local allowed_per_field
  allowed_per_field=$((HEAVY_HITTER_LIMIT * HH_CONTEXT_LIMIT))
  echo "allowed_heavy_hitter_context_rows_per_field=${allowed_per_field}"

  if [[ "$HH_CONTEXT_LIMIT" = "0" ]]; then
    allowed_per_field=0
  fi

  local violating_fields
  violating_fields="$(sqlite3 -readonly "$DB" "
SELECT COUNT(*)
FROM (
  SELECT field_profile_id, COUNT(*) AS rows
  FROM prof_field_value_sample
  WHERE sample_kind = 'heavy_hitter_context'
  GROUP BY field_profile_id
  HAVING rows > $allowed_per_field
);")"

  echo "field_profiles_exceeding_hh_context_bound=${violating_fields:-unknown}"
  if [[ "${violating_fields:-0}" != "0" ]]; then
    risk "heavy_hitter_context rows exceed expected per-field bound"
  fi

  subsection "duplicate heavy_hitter_context rows per field/value"
  sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT
  field_profile_id,
  value_hash,
  COUNT(*) AS rows
FROM prof_field_value_sample
WHERE sample_kind = 'heavy_hitter_context'
GROUP BY field_profile_id, value_hash
HAVING COUNT(*) > CASE WHEN $HH_CONTEXT_LIMIT > 0 THEN $HH_CONTEXT_LIMIT ELSE 0 END
ORDER BY rows DESC
LIMIT $TOP_N;
SQL
}

check_sample_bounds() {
  section "Sample bound checks"

  if table_exists prof_field_value_sample; then
    subsection "value priority samples exceeding per-field limit"
    local value_priority_violations
    value_priority_violations="$(sqlite3 -readonly "$DB" "
SELECT COUNT(*)
FROM (
  SELECT field_profile_id, COUNT(*) AS rows
  FROM prof_field_value_sample
  WHERE sample_kind = 'priority_sample'
  GROUP BY field_profile_id
  HAVING rows > $VALUE_SAMPLE_LIMIT
);")"
    echo "value_priority_violations=${value_priority_violations:-unknown}"
    if [[ "${value_priority_violations:-0}" != "0" ]]; then
      risk "value priority samples exceed configured per-field limit"
      sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT field_profile_id, COUNT(*) AS rows
FROM prof_field_value_sample
WHERE sample_kind = 'priority_sample'
GROUP BY field_profile_id
HAVING rows > $VALUE_SAMPLE_LIMIT
ORDER BY rows DESC
LIMIT $TOP_N;
SQL
    fi
  fi

  if table_exists prof_object_sample; then
    subsection "object priority samples exceeding scope limits"
    sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
WITH limits(scope, lim) AS (
  VALUES
    ('canonical_path', $OBJECT_CANONICAL_PRIORITY_LIMIT),
    ('site_path', $OBJECT_SITE_PRIORITY_LIMIT),
    ('field_set', $OBJECT_FIELD_SET_PRIORITY_LIMIT),
    ('type_set', $OBJECT_TYPE_SET_PRIORITY_LIMIT)
),
grouped AS (
  SELECT sample_scope, sample_key, COUNT(*) AS rows
  FROM prof_object_sample
  WHERE sample_kind = 'priority_sample'
  GROUP BY sample_scope, sample_key
)
SELECT grouped.sample_scope, grouped.sample_key, grouped.rows, limits.lim
FROM grouped
JOIN limits ON limits.scope = grouped.sample_scope
WHERE grouped.rows > limits.lim
ORDER BY grouped.rows DESC
LIMIT $TOP_N;
SQL

    local object_priority_violations
    object_priority_violations="$(sqlite3 -readonly "$DB" "
WITH limits(scope, lim) AS (
  VALUES
    ('canonical_path', $OBJECT_CANONICAL_PRIORITY_LIMIT),
    ('site_path', $OBJECT_SITE_PRIORITY_LIMIT),
    ('field_set', $OBJECT_FIELD_SET_PRIORITY_LIMIT),
    ('type_set', $OBJECT_TYPE_SET_PRIORITY_LIMIT)
),
grouped AS (
  SELECT sample_scope, sample_key, COUNT(*) AS rows
  FROM prof_object_sample
  WHERE sample_kind = 'priority_sample'
  GROUP BY sample_scope, sample_key
)
SELECT COUNT(*)
FROM grouped
JOIN limits ON limits.scope = grouped.sample_scope
WHERE grouped.rows > limits.lim;
")"
    echo "object_priority_violations=${object_priority_violations:-unknown}"
    if [[ "${object_priority_violations:-0}" != "0" ]]; then
      risk "object priority samples exceed configured scope limit"
    fi
  fi
}

print_shape_diagnostics() {
  section "Shape diagnostics"

  if table_exists prof_shape; then
    subsection "top canonical paths"
    sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT canonical_path, COUNT(*) AS shapes, SUM(object_count) AS objects
FROM prof_shape
GROUP BY canonical_path
ORDER BY shapes DESC, objects DESC
LIMIT $TOP_N;
SQL

    subsection "site paths with many shapes"
    sqlite3 -readonly "$DB" <<SQL
.headers on
.mode column
SELECT site_path, COUNT(*) AS shapes, SUM(object_count) AS objects
FROM prof_shape
GROUP BY site_path
ORDER BY shapes DESC, objects DESC
LIMIT $TOP_N;
SQL
  fi
}

print_risk_summary() {
  section "Risk summary"

  echo "risk_count=$risk_count"

  if [[ "$risk_count" -eq 0 ]]; then
    echo "status=ok"
  else
    echo "status=risk"
  fi

  if [[ "$FAIL_ON_RISK" -eq 1 && "$risk_count" -ne 0 ]]; then
    exit 2
  fi
}

parse_args "$@"

print_file_sizes
print_schema_contract
print_row_counts
print_dbstat
print_samples
print_value_distribution
check_heavy_hitter_context
check_sample_bounds
print_shape_diagnostics
print_risk_summary
