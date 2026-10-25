#!/usr/bin/env bash
# Exercise real child processes: a background failure must fail after all waits.
set -euo pipefail
repository_root="$(git rev-parse --show-toplevel)"
test_directory="$(mktemp -d "${TMPDIR:-/tmp}/workflow-timing.XXXXXX")"
trap 'rm -f "$test_directory/step.sh" "$test_directory/after-failure"; rmdir "$test_directory"' EXIT
export RUNNER_TEMP="$test_directory"
export GITHUB_JOB=local GITHUB_ACTION=local-step

printf 'exit 7\n' > "$test_directory/step.sh"
if bash -e "$repository_root/scripts/tooling/time-step.sh" "$test_directory/step.sh"; then
  echo 'failed step was accepted' >&2
  exit 1
else
  [[ "$?" -eq 7 ]]
fi
[[ "$(cut -f2 "$test_directory/workflow-timings/local/local-step.tsv")" -eq 7 ]]

source "$repository_root/scripts/tooling/parallel-steps.sh"
# The first child waits for the second child, proving actual overlap. The second
# records its effect even though the first fails; wait_steps must reap both.
# shellcheck disable=SC2016 # The child shell expands its own positional argument.
start_step failure bash -c 'while [[ ! -e "$1" ]]; do if (( SECONDS >= 5 )); then exit 8; fi; done; exit 9' bash "$test_directory/after-failure"
start_step success touch "$test_directory/after-failure"
if wait_steps; then
  echo 'failed background command was accepted' >&2
  exit 1
fi
[[ -f "$test_directory/after-failure" ]]
[[ "$(cut -f2 "$parallel_step_directory/failure.tsv")" -eq 9 ]]
[[ "$(cut -f2 "$parallel_step_directory/success.tsv")" -eq 0 ]]

parallel_step_pids=()
parallel_step_names=()
start_step success true
wait_steps
parsed_timing="$(printf 'job\tstep\tdate ^[[32;1m PASS^[[0m [ 1.250s] ( 1/2) crate::suite case\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'1.250\tPASS\texample\tcrate::suite case' ]]
parsed_timing="$(printf '0.003s test_case (suite.Class.test_case)\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'0.003\tMEASURED\texample\ttest_case (suite.Class.test_case)' ]]
parsed_timing="$(printf 'job\tstep\tdate TRANSPORT_TEST\t0\t0\tidle_provider_transport_stops_scheduling_itself\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'0\tPASS\texample\tidle_provider_transport_stops_scheduling_itself' ]]
printf 'Workflow timing checks: 6 passed\n'
# Remove only this test's generated files, after every child has exited.
rm -f "$test_directory/workflow-timings/local/local-step.tsv" "$parallel_step_directory/failure.tsv" "$parallel_step_directory/success.tsv" "$parallel_step_directory/failure.log" "$parallel_step_directory/success.log"
rmdir "$test_directory/workflow-timings/local" "$test_directory/workflow-timings" "$parallel_step_directory" "$test_directory/workflow-parallel"
