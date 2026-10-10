#!/usr/bin/env bash
# Runs a command in CI. If it fails, the first error lines are also
# reported as GitHub annotations, which can be read through the API even
# where the raw job logs cannot be downloaded.
set -uo pipefail
log=$(mktemp)
"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
if [ "$status" -ne 0 ]; then
  grep -E '(^|: )error(\[E[0-9]+\])?:|panicked at|^test .* FAILED|^ERROR|^Error' "$log" | head -10 |
    while IFS= read -r line; do echo "::error::${line//%/%25}"; done
  echo "::error::command failed ($status): $*"
fi
rm -f "$log"
exit "$status"
