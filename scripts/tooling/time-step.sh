#!/usr/bin/env bash
# GitHub custom shell: preserve the step's exit status and report elapsed time.
set -uo pipefail

step_script="$1"
step_name="${GITHUB_ACTION:-local-step}"
timing_directory="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/workflow-timings/${GITHUB_JOB:-local}"
mkdir -p "$timing_directory"
started_seconds="$(date +%s)"
if bash --noprofile --norc -e -o pipefail "$step_script"; then
  step_status=0
else
  step_status=$?
fi
elapsed_seconds="$(( $(date +%s) - started_seconds ))"
printf '%s\t%s\t%s\n' "$elapsed_seconds" "$step_status" "$step_name" > "$timing_directory/${step_name//\//-}.tsv"
printf '\nStep %s: %ss, exit %s\n' "$step_name" "$elapsed_seconds" "$step_status"
exit "$step_status"
