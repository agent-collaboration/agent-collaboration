#!/usr/bin/env bash
# One shared Cargo target directory; only artifact consumers run concurrently.
set -euo pipefail
script_directory="$(cd "$(dirname "$0")" && pwd)"
artifact_directory="${RUNNER_TEMP:?RUNNER_TEMP must be set}/release-build"
mkdir -p "$artifact_directory"

# Build the small optimized regression target and exporter first. Starting
# multiple Cargo compiles in the background just serializes on Cargo's lock.
cargo test --locked --release -p acp-client-runtime --test provider_transport_idle \
  --no-run --message-format=json --timings > "$artifact_directory/transport-build.jsonl"
transport_binary="$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "provider_transport_idle" and .executable != null) | .executable' "$artifact_directory/transport-build.jsonl")"
[[ -x "$transport_binary" ]]
transport_tests="$("$transport_binary" --list | sed -n 's/: test$//p')"
[[ -n "$transport_tests" ]]
cargo build --locked --release -p collaboration-protocol --example export_protocol_types --timings

run_transport_tests() {
  local test_name started_seconds test_status failed_tests=0
  while IFS= read -r test_name; do
    started_seconds="$(date +%s)"
    if "$transport_binary" --exact "$test_name" --nocapture; then
      test_status=0
    else
      test_status=$?
      failed_tests=$((failed_tests + 1))
    fi
    printf 'TRANSPORT_TEST\t%s\t%s\t%s\n' "$(( $(date +%s) - started_seconds ))" "$test_status" "$test_name"
  done <<< "$transport_tests"
  [[ "$failed_tests" -eq 0 ]]
}

export_release_schemas() {
  mkdir -p target/protocol-schemas/communication-control target/protocol-schemas/acp-protocol || return
  target/release/examples/export_protocol_types > target/protocol-schemas/communication-control/protocol-types.json || return
  cp protocol-schemas/acp-protocol/stable-schema.json protocol-schemas/acp-protocol/upstream-license.txt target/protocol-schemas/acp-protocol/ || return
  cp protocol-schemas/schema-packaging.md target/protocol-schemas/
}

reject_test_keychain_support() {
  local dependency_features
  dependency_features="$(cargo tree --locked -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade -e normal,build,features -i codex-router-secret-store)" || return
  if grep -q 'keychain-test-guard' <<<"$dependency_features"; then
    echo 'release dependency graph includes keychain-test-guard' >&2
    return 1
  fi
}

# shellcheck source=scripts/tooling/parallel-steps.sh
source "$script_directory/parallel-steps.sh"
start_step release-binaries cargo build --locked --release -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade --bins --timings
start_step optimized-transport run_transport_tests
start_step protocol-schemas export_release_schemas
start_step production-features reject_test_keychain_support
wait_steps
