# Testing the debug Host locally

Use the existing `codex-router-debug` profile and fresh test threads. The debug Host selects `gpt-5.6-luna` in memory, keeps normal Codex home, and puts its sockets and workspace in a new private directory under `/tmp`. It disables home hooks only in its owned test app-server so they cannot inject extra work. It uses the existing debug router credentials. It never edits the home profile or replaces the production router.

## Build once

Run from the repository root:

```sh
cargo build -p codex-router-cli --bin codex-router
cargo build -p codex-router-debug-hosts --bin automation-debug-host
```

Build it without features for a real acceptance Host. The debug-hosts crate's
`runtime-test-support` feature adds test credential features; such a build suits
regression tests, not real credential observation.
Before runtime admission, inspect Cargo's actual helper, Host, and secret-store
artifact features and reject any test credential features.

## Start the debug Host

In one terminal, choose a previously unused name:

```sh
proof_root=/tmp/luna-proof-my-check
./target/debug/automation-debug-host \
  --run-directory "$proof_root" \
  --router-binary "$PWD/target/debug/codex-router" \
  --port 18787
```

Keep this foreground process running. An existing directory, occupied port, production port, alternate Codex home or invalid debug provider is rejected. The existing debug profile must point at the chosen debug port.

`debugHostPrepared` identifies the paths and Luna selection. It precedes startup readiness. In a second terminal, using the same `proof_root` value, wait for `agent-communication/service.json` and inspect readiness:

```sh
proof_root=/tmp/luna-proof-my-check
./target/debug/codex-router host status \
  --router-root "$proof_root" --port 18787 --require-debug-isolation
./target/debug/agent-collaboration endpoints list \
  --service-directory "$proof_root/agent-communication" --json
```

The Host should report `OwnedReachable` and `NativeReady`; endpoint discovery must include a native generation and schema digest. `LocalReadyRemoteDegraded` is expected with Remote Control disabled for this local debug setup. A prepared marker alone does not establish readiness.

The outer test process needs permission to use the debug Unix sockets and normal Codex state.

## Check the control-socket permission boundary

An opt-in OS test verifies the socket permission boundary without a
model or app-server:

```sh
cargo test -p agent-collaboration --test integration \
  native_sandbox_access::codex_sandbox_requires_exact_control_socket_permission -- --ignored --exact --nocapture
```

Stop the foreground debug Host with Ctrl-C when finished. It shuts down its retained children. The private test artifacts remain for inspection, and Codex retains its ordinary session records. No directory deletion or production restart is part of this procedure.

## Check Claude routed-session discovery and preflight

Use a fresh private Router root, isolated `HOME` and `CODEX_HOME`, and unused loopback ports. This keeps the
check away from the installed Host and the owner's Claude session registry. Build the debug binaries from
the repository root:

```sh
cargo build -p codex-router-cli --bin codex-router \
  -p agent-collaboration --bin agent-sessions
```

Create a new root and its isolated homes and socket directory:

```sh
claude_proof_root="$(mktemp -d /tmp/claude-routing-proof.XXXXXX)"
mkdir -p "$claude_proof_root/home" "$claude_proof_root/codex-home" \
  "$claude_proof_root/native-socket"
chmod 700 "$claude_proof_root" "$claude_proof_root/home" \
  "$claude_proof_root/codex-home" "$claude_proof_root/native-socket"
```

In a foreground terminal, start the Host on two unused loopback ports. The Router proxy port below is an
example; use the same port consistently for this run:

```sh
env HOME="$claude_proof_root/home" CODEX_HOME="$claude_proof_root/codex-home" \
  CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET="$claude_proof_root/native-socket/app-server.sock" \
  ./target/debug/codex-router host --router-root "$claude_proof_root" \
  --port 43131 --mcp-bind 127.0.0.1:43132 --require-debug-isolation
```

The Host owns this unused Router port, provisions the local token before starting `serve`, and publishes its
configured proxy endpoint in `agent-communication/service.json`. Confirm the endpoint before running the
session commands:

```sh
rg -o '"routerProxyEndpoint":"[^"]+"' \
  "$claude_proof_root/agent-communication/service.json"
```

In another terminal, list Claude sessions and inspect the launch arguments. The debug root environment makes
`agent-sessions` use the same service directory as the Host:

```sh
env HOME="$claude_proof_root/home" CODEX_HOME="$claude_proof_root/codex-home" \
  CODEX_ROUTER_DEBUG_ROUTER_ROOT="$claude_proof_root" \
  ./target/debug/agent-sessions --provider claude --list --format json

env HOME="$claude_proof_root/home" CODEX_HOME="$claude_proof_root/codex-home" \
  CODEX_ROUTER_DEBUG_ROUTER_ROOT="$claude_proof_root" \
  ./target/debug/agent-sessions --provider claude --new --dry-run
```

The list combines active Router sessions with stored transcript metadata under the isolated `HOME`; it does
not read transcript content. Dry-run prints the `claude` command and does not contact Router or start Claude.
To check the stopped-Router boundary, stop this foreground Host with Ctrl-C, then run a non-dry routed launch
with the same environment and confirm it exits nonzero with the endpoint-not-published or Router-not-ready
error before Claude starts. The real-client request path through a fake Anthropic upstream remains the PR7
acceptance proof; listing and dry-run do not establish that request path.

## Automated gates

The permanent SQLite, filesystem, CLI and scripted-native tests exercise failure/recovery cases without paid model calls. They remain distinct from the opt-in live journey above.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
cargo audit
python3 -m unittest scripts.tests.test_update_homebrew_formula -v
```

CI also builds the router with all features, checks an isolated Cargo installation, and runs the quota-reset PTY harness. Preserve those gates when preparing the PR.

Source: [debug Host launcher](../../crates/codex-router-debug-hosts/src/bin/automation-debug-host.rs), [Claude launch target](../../crates/agent-session-inventory/src/session_commands/claude_launch_target.rs), [Claude launch target tests](../../crates/agent-session-inventory/src/session_commands/claude_launch_target_tests.rs), [Host Claude launch environment](../../crates/codex-router-host/src/claude_provider_launch_environment.rs), [Host token startup](../../crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs).
