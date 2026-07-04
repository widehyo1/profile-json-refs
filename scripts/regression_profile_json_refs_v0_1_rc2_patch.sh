#!/usr/bin/env bash
# Regression script for the profile-json-refs v0.1.0-rc.2 performance-safe candidate.
#
# Purpose:
#   Verify that a v0.1.x performance patch does not change the public contract:
#   - same approved prof_* table set
#   - no views
#   - no forbidden run/manifest/warning/array tables
#   - no stdin support
#   - --strict remains unsupported
#   - stdout remains summary-only
#   - --quiet suppresses stdout
#   - --perf-log goes to stderr
#
# It also verifies the rc.2 performance fix expectation:
#   - heavy_hitter_context is disabled by default / configured as 0
#   - high-cardinality fields do not produce heavy_hitter_context rows
#   - prof_field_summary and prof_field_value are finalized
#
# Required:
#   - profile-json-refs candidate binary
#   - dump-json-refs binary to create refs/schemas.sqlite for the fixture
#
# Usage:
#   PROFILE_JSON_REFS_BIN=target/release/profile-json-refs \
#   DUMP_JSON_REFS_BIN=dump-json-refs \
#   ./regression_profile_json_refs_v0_1_rc2_patch.sh
#
# Optional:
#   BASELINE_PROFILE_JSON_REFS_BIN=/path/to/v0.1.0/profile-json-refs
#     When provided, the script compares schema signatures only.
#     Row counts are not compared because the performance patch may reduce samples.

set -euo pipefail

PROFILE_BIN="${PROFILE_JSON_REFS_BIN:-}"
DUMP_BIN="${DUMP_JSON_REFS_BIN:-dump-json-refs}"
BASELINE_BIN="${BASELINE_PROFILE_JSON_REFS_BIN:-}"

KEEP=0
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-180}"
TMPDIR_PARENT="${TMPDIR:-/tmp}"
work=""

usage() {
  cat <<'EOF'
Usage:
  regression_profile_json_refs_v0_1_rc2_patch.sh [OPTIONS]

Environment:
  PROFILE_JSON_REFS_BIN
      Candidate profile-json-refs binary. Required unless --profile-bin is used.

  DUMP_JSON_REFS_BIN
      dump-json-refs binary. Default: dump-json-refs

  BASELINE_PROFILE_JSON_REFS_BIN
      Optional v0.1.0 binary for schema-signature comparison.

  TIMEOUT_SECONDS
      Command timeout. Default: 180.

Options:
  --profile-bin <PATH>
      Candidate profile-json-refs binary.

  --dump-bin <PATH>
      dump-json-refs binary.

  --baseline-bin <PATH>
      Optional baseline profile-json-refs binary.

  --keep
      Keep temp directory.

  -h, --help
      Show this help.
EOF
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

log() {
  echo "[regression] $*" >&2
}

have_timeout() {
  command -v timeout >/dev/null 2>&1
}

run_cmd() {
  if have_timeout; then
    timeout "$TIMEOUT_SECONDS" "$@"
  else
    "$@"
  fi
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --profile-bin)
        PROFILE_BIN="${2:?--profile-bin requires a value}"
        shift 2
        ;;
      --dump-bin)
        DUMP_BIN="${2:?--dump-bin requires a value}"
        shift 2
        ;;
      --baseline-bin)
        BASELINE_BIN="${2:?--baseline-bin requires a value}"
        shift 2
        ;;
      --keep)
        KEEP=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown argument: $1"
        ;;
    esac
  done

  [[ -n "$PROFILE_BIN" ]] || die "PROFILE_JSON_REFS_BIN or --profile-bin is required"
  command -v "$PROFILE_BIN" >/dev/null 2>&1 || [[ -x "$PROFILE_BIN" ]] || die "profile binary not executable/found: $PROFILE_BIN"
  command -v "$DUMP_BIN" >/dev/null 2>&1 || [[ -x "$DUMP_BIN" ]] || die "dump-json-refs binary not executable/found: $DUMP_BIN"
  command -v sqlite3 >/dev/null 2>&1 || die "sqlite3 is required"
}

sql_scalar() {
  local db="$1"
  local query="$2"
  sqlite3 -readonly "$db" "$query"
}

assert_eq() {
  local expected="$1"
  local actual="$2"
  local msg="$3"
  if [[ "$expected" != "$actual" ]]; then
    echo "ASSERT FAIL: $msg" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
}

assert_gt_zero() {
  local value="$1"
  local msg="$2"
  if [[ "$value" -le 0 ]]; then
    echo "ASSERT FAIL: $msg" >&2
    echo "  actual: $value" >&2
    exit 1
  fi
}

assert_file_exists() {
  [[ -f "$1" ]] || die "expected file does not exist: $1"
}

schema_signature() {
  local db="$1"
  sqlite3 -readonly "$db" <<'SQL'
.mode list
.separator |
SELECT 'TABLE', name
FROM sqlite_master
WHERE type='table' AND name LIKE 'prof_%'
UNION ALL
SELECT 'COLUMN', m.name || '.' || p.name || ':' || p.type || ':' || p."notnull" || ':' || COALESCE(p.dflt_value, '')
FROM sqlite_master AS m
JOIN pragma_table_info(m.name) AS p
WHERE m.type='table' AND m.name LIKE 'prof_%'
ORDER BY 1, 2;
SQL
}

validate_contract_db() {
  local db="$1"
  log "validate profile sqlite contract: $db"

  local approved
  approved="$(sqlite3 -readonly "$db" <<'SQL'
.mode list
SELECT name
FROM sqlite_master
WHERE type='table' AND name LIKE 'prof_%'
ORDER BY name;
SQL
)"

  local expected
  expected="$(cat <<'EOF'
prof_field_summary
prof_field_value
prof_field_value_sample
prof_object_sample
prof_shape
prof_shape_field
prof_source_summary
EOF
)"
  assert_eq "$expected" "$approved" "approved prof_* table set changed"

  local view_count
  view_count="$(sql_scalar "$db" "SELECT COUNT(*) FROM sqlite_master WHERE type='view';")"
  assert_eq "0" "$view_count" "v0.1.x must not create SQLite views"

  local forbidden
  forbidden="$(sqlite3 -readonly "$db" <<'SQL'
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
  );
SQL
)"
  assert_eq "0" "$forbidden" "forbidden tables exist"

  local summary_rows
  summary_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_source_summary;")"
  assert_eq "1" "$summary_rows" "prof_source_summary must contain exactly one row"

  local shape_rows field_rows field_summary_rows field_value_rows
  shape_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_shape;")"
  field_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_shape_field;")"
  field_summary_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_field_summary;")"
  field_value_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_field_value;")"

  assert_gt_zero "$shape_rows" "prof_shape should be populated"
  assert_gt_zero "$field_rows" "prof_shape_field should be populated"
  assert_gt_zero "$field_summary_rows" "prof_field_summary should be populated"
  assert_gt_zero "$field_value_rows" "prof_field_value should be populated"

  local hh_context_rows
  hh_context_rows="$(sql_scalar "$db" "SELECT COUNT(*) FROM prof_field_value_sample WHERE sample_kind='heavy_hitter_context';")"
  assert_eq "0" "$hh_context_rows" "heavy_hitter_context should be disabled by default/configured zero in rc.2 regression"

  local value_priority_violations
  value_priority_violations="$(sqlite3 -readonly "$db" <<'SQL'
SELECT COUNT(*)
FROM (
  SELECT field_profile_id, COUNT(*) AS rows
  FROM prof_field_value_sample
  WHERE sample_kind='priority_sample'
  GROUP BY field_profile_id
  HAVING rows > 2
);
SQL
)"
  assert_eq "0" "$value_priority_violations" "value priority samples exceed regression config limit"

  local empty_string_col
  empty_string_col="$(sqlite3 -readonly "$db" "SELECT COUNT(*) FROM pragma_table_info('prof_field_summary') WHERE name='empty_string_count';")"
  assert_eq "1" "$empty_string_col" "prof_field_summary.empty_string_count column missing"

  local empty_string_count
  empty_string_count="$(sqlite3 -readonly "$db" "SELECT COALESCE(SUM(empty_string_count), 0) FROM prof_field_summary;")"
  assert_gt_zero "$empty_string_count" "empty string should be counted as string fact"

  local exact_rows approx_rows
  exact_rows="$(sqlite3 -readonly "$db" "SELECT COUNT(*) FROM prof_field_value WHERE value_source='exact_full';")"
  approx_rows="$(sqlite3 -readonly "$db" "SELECT COUNT(*) FROM prof_field_summary WHERE distinct_count_method='approximate';")"

  assert_gt_zero "$exact_rows" "fixture should produce exact_full rows"
  assert_gt_zero "$approx_rows" "fixture should produce approximate field summaries under low threshold config"
}

validate_stdout_stderr() {
  local stdout="$1"
  local stderr="$2"

  grep -q '^profile-json-refs: wrote ' "$stdout" || die "stdout missing wrote line"
  grep -q '^documents: ' "$stdout" || die "stdout missing documents summary"
  grep -q '^objects: ' "$stdout" || die "stdout missing objects summary"
  grep -q '^shapes: ' "$stdout" || die "stdout missing shapes summary"
  grep -q '^field_profiles: ' "$stdout" || die "stdout missing field_profiles summary"
  grep -q '^elapsed: ' "$stdout" || die "stdout missing elapsed line"

  if grep -q 'prof_field_value_sample' "$stdout"; then
    die "stdout appears to contain detailed table rows; contract requires summary-only stdout"
  fi

  grep -q '\[perf\]' "$stderr" || die "--perf-log did not emit [perf] lines to stderr"
}

make_fixture() {
  local dir="$1"
  local input="$dir/fixture.jsonl"

  : > "$input"

  # exact/categorical rows
  cat >> "$input" <<'EOF'
{"id":"id-0001","status":"A","code":null,"payload":{},"items":[{"kind":"amount","value":100},{"kind":"code","value":"A"}]}
{"id":"id-0002","status":"B","code":"","payload":{"name":"first"},"items":[{"kind":"amount","value":"200"},{"kind":"error","message":"invalid"}]}
{"id":"id-0003","status":"A","code":"X","payload":{"name":"second","extra":true},"items":[{"kind":"amount","value":300},{"kind":"code","value":"B"}]}
EOF

  # high-cardinality rows to force exact fallback with low threshold config
  for i in $(seq 1 80); do
    printf '{"id":"hc-%04d","status":"%s","code":"C%02d","payload":{"n":%d,"flag":%s},"items":[{"kind":"amount","value":%d},{"kind":"code","value":"%s"}]}\n' \
      "$i" \
      "$([[ $((i % 2)) -eq 0 ]] && echo A || echo B)" \
      "$((i % 7))" \
      "$i" \
      "$([[ $((i % 2)) -eq 0 ]] && echo true || echo false)" \
      "$i" \
      "K$((i % 5))" >> "$input"
  done

  echo "$input"
}

write_config() {
  local path="$1"
  cat > "$path" <<'YAML'
sampling:
  object:
    sample_json_limit_bytes: 2048
    canonical_path:
      priority_sample_limit: 1
    site_path:
      priority_sample_limit: 1
    field_set:
      priority_sample_limit: 1
    type_set:
      priority_sample_limit: 1
  value:
    value_json_limit_bytes: 256
    parent_object_json_limit_bytes: 512
    priority_sample_limit_per_field_profile: 2
    heavy_hitter_context_sample_limit: 0

value_profile:
  value_text_limit_bytes: 128
  exact_distinct_threshold: 16
  exact_value_bytes_per_field_profile: 4096
  global_exact_value_bytes_budget: 65536
  hll_precision: 10
  heavy_hitter_limit: 8

flush:
  chunk_object_sample_rows: 50
  chunk_value_sample_rows: 50
  chunk_shape_rows: 50
  chunk_field_rows: 100
YAML
}

run_profile_case() {
  local profile_bin="$1"
  local case_dir="$2"
  local label="$3"

  local input="$case_dir/fixture.jsonl"
  local refs_db="$case_dir/refs/schemas.sqlite"
  local config="$case_dir/profile-config.yaml"
  local out="$case_dir/profile-$label.sqlite"
  local stdout="$case_dir/profile-$label.stdout"
  local stderr="$case_dir/profile-$label.stderr"

  log "generate refs for $label"
  (
    cd "$case_dir"
    run_cmd "$DUMP_BIN" "$input" --jsonl > "dump-$label.stdout" 2> "dump-$label.stderr"
  )

  assert_file_exists "$refs_db"
  write_config "$config"

  log "run profile-json-refs $label"
  run_cmd "$profile_bin" "$input" \
    --jsonl \
    --refs "$refs_db" \
    --out "$out" \
    --config "$config" \
    --perf-log \
    > "$stdout" \
    2> "$stderr"

  assert_file_exists "$out"
  validate_stdout_stderr "$stdout" "$stderr"
  validate_contract_db "$out"

  # quiet mode must produce no normal stdout.
  local quiet_out="$case_dir/profile-$label-quiet.sqlite"
  local quiet_stdout="$case_dir/profile-$label-quiet.stdout"
  local quiet_stderr="$case_dir/profile-$label-quiet.stderr"

  run_cmd "$profile_bin" "$input" \
    --jsonl \
    --refs "$refs_db" \
    --out "$quiet_out" \
    --config "$config" \
    --quiet \
    > "$quiet_stdout" \
    2> "$quiet_stderr"

  if [[ -s "$quiet_stdout" ]]; then
    echo "quiet stdout:" >&2
    cat "$quiet_stdout" >&2
    die "--quiet produced stdout"
  fi

  echo "$out"
}

validate_cli_negative_contracts() {
  local profile_bin="$1"
  local work="$2"

  log "validate CLI negative contracts"

  if "$profile_bin" --help 2>&1 | grep -q -- '--strict'; then
    die "--strict appears in --help; v0.1.x contract excludes it"
  fi

  if "$profile_bin" - --jsonl > "$work/stdin.stdout" 2> "$work/stdin.stderr"; then
    die "profile-json-refs accepted '-' stdin input; v0.1.x must reject stdin"
  fi

  if "$profile_bin" "$work/fixture.jsonl" --strict > "$work/strict.stdout" 2> "$work/strict.stderr"; then
    die "profile-json-refs accepted --strict; v0.1.x must reject it"
  fi
}

compare_baseline_schema_if_present() {
  local candidate_db="$1"
  local work="$2"

  if [[ -z "$BASELINE_BIN" ]]; then
    return
  fi

  command -v "$BASELINE_BIN" >/dev/null 2>&1 || [[ -x "$BASELINE_BIN" ]] || die "baseline binary not executable/found: $BASELINE_BIN"

  local baseline_dir="$work/baseline"
  mkdir -p "$baseline_dir"
  cp "$work/fixture.jsonl" "$baseline_dir/fixture.jsonl"

  (
    cd "$baseline_dir"
    run_cmd "$DUMP_BIN" "$baseline_dir/fixture.jsonl" --jsonl > dump-baseline.stdout 2> dump-baseline.stderr
  )

  write_config "$baseline_dir/profile-config.yaml"

  # If baseline does not accept the new safer config, fall back to no config for schema signature only.
  if ! run_cmd "$BASELINE_BIN" "$baseline_dir/fixture.jsonl" \
      --jsonl \
      --refs "$baseline_dir/refs/schemas.sqlite" \
      --out "$baseline_dir/profile-baseline.sqlite" \
      --config "$baseline_dir/profile-config.yaml" \
      > "$baseline_dir/profile-baseline.stdout" \
      2> "$baseline_dir/profile-baseline.stderr"; then
    log "baseline failed with rc.2 config; retry without config for schema signature"
    run_cmd "$BASELINE_BIN" "$baseline_dir/fixture.jsonl" \
      --jsonl \
      --refs "$baseline_dir/refs/schemas.sqlite" \
      --out "$baseline_dir/profile-baseline.sqlite" \
      > "$baseline_dir/profile-baseline.stdout" \
      2> "$baseline_dir/profile-baseline.stderr"
  fi

  local baseline_sig="$baseline_dir/schema.baseline.txt"
  local candidate_sig="$baseline_dir/schema.candidate.txt"

  schema_signature "$baseline_dir/profile-baseline.sqlite" > "$baseline_sig"
  schema_signature "$candidate_db" > "$candidate_sig"

  if ! diff -u "$baseline_sig" "$candidate_sig" > "$baseline_dir/schema.diff"; then
    echo "Schema signature diff:" >&2
    cat "$baseline_dir/schema.diff" >&2
    die "candidate schema signature differs from baseline"
  fi

  log "baseline schema signature matches candidate"
}

main() {
  parse_args "$@"

  work="$(mktemp -d "$TMPDIR_PARENT/profile-json-refs-rc2-regression.XXXXXX")"

  if [[ "$KEEP" -eq 0 ]]; then
    trap 'rm -rf "$work"' EXIT
  else
    log "keeping temp dir: $work"
  fi

  log "work dir: $work"
  make_fixture "$work" >/dev/null

  validate_cli_negative_contracts "$PROFILE_BIN" "$work"

  local candidate_db
  candidate_db="$(run_profile_case "$PROFILE_BIN" "$work" "candidate")"

  compare_baseline_schema_if_present "$candidate_db" "$work"

  log "PASS v0.1.0-rc.2 performance regression"
  log "candidate profile: $candidate_db"
}

main "$@"
