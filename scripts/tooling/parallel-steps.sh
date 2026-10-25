#!/usr/bin/env bash
# Source this file, start independent commands, then wait for every result.
# Compile shared Cargo targets before launching commands that consume them.
parallel_step_pids=()
parallel_step_names=()
parallel_step_directory="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/workflow-parallel/${GITHUB_JOB:-local}"
mkdir -p "$parallel_step_directory"

start_step() {
  local step_name="$1"
  shift
  (
    started_seconds="$(date +%s)"
    if "$@"; then
      step_status=0
    else
      step_status=$?
    fi
    elapsed_seconds="$(( $(date +%s) - started_seconds ))"
    printf '%s\t%s\t%s\n' "$elapsed_seconds" "$step_status" "$step_name" > "$parallel_step_directory/$step_name.tsv"
    exit "$step_status"
  ) > "$parallel_step_directory/$step_name.log" 2>&1 &
  parallel_step_pids+=("$!")
  parallel_step_names+=("$step_name")
}

wait_steps() {
  local failed_steps=0 step_index step_status step_name
  for step_index in "${!parallel_step_pids[@]}"; do
    step_name="${parallel_step_names[$step_index]}"
    if wait "${parallel_step_pids[$step_index]}"; then
      step_status=0
    else
      step_status=$?
      failed_steps=$((failed_steps + 1))
    fi
    printf '::group::%s (exit %s)\n' "$step_name" "$step_status"
    cat "$parallel_step_directory/$step_name.log"
    if [[ -f "$parallel_step_directory/$step_name.tsv" ]]; then
      cat "$parallel_step_directory/$step_name.tsv"
    fi
    printf '::endgroup::\n'
  done
  [[ "$failed_steps" -eq 0 ]]
}
