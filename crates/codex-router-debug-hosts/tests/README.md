# Debug Host test contracts

The six bin targets enable their colocated unit tests. Run the existing Host
launcher unit tests with
`cargo nextest run -p codex-router-debug-hosts --features runtime-test-support`.
They cover automation launch options, permissions and diagnostics; board Host
argument parsing; acceptance ownership and proof settings; and imagegen audit
and profile projection. The provider-only launcher has no unit cases.

The `e2e` aggregate requires `signin-test-support`. Its
`central_signin_launcher` journey starts the compiled
`CARGO_BIN_EXE_central_sign_in_launcher` as a separate owned process. It checks
the exact localhost/socket readiness envelope, socket mode 0600, typed stdin
and stderr IPC, unknown-org observation without creating a store, real signed
provisioning and person-proof/floor rows, one clock reaching a renewal and a
new org, refusal of an unknown command and malformed time command followed by
live valid commands, SIGTERM and SIGINT with stdin still open, socket removal,
and an external fresh-root restart. Signed statement and observation comparisons never print
their values on failure. Child failure cleanup uses `kill_on_drop`; successful
shutdown explicitly waits and reaps. Event waits use the existing nextest case
budget, with no wall-clock sleeps or test timeouts.

Run that aggregate with
`cargo nextest run -p codex-router-debug-hosts --features signin-test-support --test e2e`.
It is a whole-process startup journey; there is no integration aggregate.
These development launchers are never dependencies of production crates. Build
the normal automation Host without features to keep test credential guards out.
