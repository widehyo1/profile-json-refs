#!/usr/bin/env bash
set -euo pipefail

out="${1:?usage: $0 refs.sqlite}"
fixture_sql="${2:-fixtures/refs/minimal_refs.sql}"

rm -f "$out"
sqlite3 "$out" < "$fixture_sql"
