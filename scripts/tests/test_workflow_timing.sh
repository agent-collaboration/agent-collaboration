#!/usr/bin/env bash
# Exercise the timing formats emitted by the real workflow test runners.
set -euo pipefail
repository_root="$(git rev-parse --show-toplevel)"
parsed_timing="$(printf 'job\tstep\tdate ^[[32;1m PASS^[[0m [ 1.250s] ( 1/2) crate::suite case\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'1.250\tPASS\texample\tcrate::suite case' ]]
parsed_timing="$(printf '0.003s     test_case (suite.Class.test_case)\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'0.003\tMEASURED\texample\ttest_case (suite.Class.test_case)' ]]
parsed_timing="$(printf 'job\tstep\tdate TRANSPORT_TEST\t0\t0\tidle_provider_transport_stops_scheduling_itself\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'0\tPASS\texample\tidle_provider_transport_stops_scheduling_itself' ]]
parsed_timing="$(printf 'job\tstep\tdate WORKERD_TEST\t0.125\tsession_scope_workerd/mailbox-capacity\n' | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "$parsed_timing" == $'0.125\tMEASURED\texample\tsession_scope_workerd/mailbox-capacity' ]]
# Timing a failing command must preserve its exit status and stop the step.
set +e
timed_output="$(TIMEFORMAT=$'WORKERD_TEST\t%3R\tfailing_example' bash -e -c 'time bash -c "exit 7"; echo unreachable' 2>&1)"
timed_status=$?
set -e
[[ "$timed_status" == 7 && "$timed_output" != *unreachable* ]]
parsed_timing="$(printf '%s\n' "$timed_output" | bash "$repository_root/scripts/tooling/extract-test-timings.sh" example)"
[[ "${parsed_timing#*$'\t'}" == $'MEASURED\texample\tfailing_example' ]]
printf 'Workflow timing parser checks: 5 passed\n'
