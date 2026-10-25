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
printf 'Workflow timing parser checks: 3 passed\n'
