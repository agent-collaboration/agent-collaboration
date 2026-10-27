#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

export PATH="${HOME}/.cargo/bin:${PATH}"

cd "${repo_root}"

if command -v cargo >/dev/null 2>&1 && cargo --version >/dev/null 2>&1; then
  cargo_command=(cargo)
elif command -v rustup >/dev/null 2>&1 && rustup run 1.95.0 cargo --version >/dev/null 2>&1; then
  cargo_command=(rustup run 1.95.0 cargo)
elif [[ -x "${HOME}/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/cargo" ]]; then
  cargo_command=("${HOME}/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin/cargo")
else
  echo "cargo or rustup with toolchain 1.95.0 is required" >&2
  exit 127
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node is required" >&2
  exit 127
fi

smoke_output="$(mktemp "${TMPDIR:-/tmp}/codex-router-plan2-installed.XXXXXX")"

"${cargo_command[@]}" test -p codex-router-proxy --lib -- upstream_usage_limit_frame --nocapture
"${cargo_command[@]}" test -p codex-router-proxy --lib -- assembled_loopback_router_runtime_retries_http_quota_errors_until_account_can_serve --nocapture
"${cargo_command[@]}" test -p codex-router-proxy --lib -- assembled_loopback_router_runtime_hides_http_quota_errors_when_all_accounts_exhausted --nocapture

set +e
tests/smoke/installed_codex_mock.sh --transport websocket --scenario quota-reconnect | tee "${smoke_output}"
status=${PIPESTATUS[0]}
set -e
if [[ "${status}" -ne 0 ]]; then
  exit "${status}"
fi

artifact_path="$(
  awk '/codex_router_quota_reconnect_artifact=/{sub(/^.*codex_router_quota_reconnect_artifact=/, ""); value=$0} END{print value}' "${smoke_output}"
)"
if [[ -z "${artifact_path}" || ! -f "${artifact_path}" ]]; then
  echo "quota reconnect smoke did not produce an artifact path" >&2
  cat "${smoke_output}" >&2
  exit 1
fi

node verification/tooling/quota-smoke-assertions.ts plan2 "${artifact_path}"

echo "quota routing plan2 observability smoke ok: artifact=${artifact_path}"
