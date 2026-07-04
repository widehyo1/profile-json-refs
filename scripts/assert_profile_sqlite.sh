#!/usr/bin/env bash
set -euo pipefail

db="${1:?usage: $0 profile.sqlite}"

sqlite3 "$db" <<'SQL'
.headers off
.mode list

SELECT 'views=' || COUNT(*) FROM sqlite_master WHERE type = 'view';

SELECT 'forbidden_tables=' || COUNT(*)
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

SELECT 'summary_rows=' || COUNT(*) FROM prof_source_summary;
SQL
