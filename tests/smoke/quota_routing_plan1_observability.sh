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

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "sqlite3 is required" >&2
  exit 127
fi

if ! command -v node >/dev/null 2>&1; then
  echo "node is required" >&2
  exit 127
fi

smoke_root="$(mktemp -d "${TMPDIR:-/tmp}/codex-router-plan1-otel.XXXXXX")"
router_root="${smoke_root}/router"
mkdir -p "${router_root}"

export CODEX_ROUTER_OBSERVABILITY_MARKER="plan1-observability-smoke"
export RUST_LOG="warn,codex_router_cli=info,codex_router_proxy=info,opentelemetry_sdk=off,opentelemetry_otlp=off"

"${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota status \
  --no-refresh \
  --router-root "${router_root}" \
  --format plain \
  --now-unix-seconds 10000 \
  >/dev/null \
  2>"${smoke_root}/bootstrap.stderr"

sqlite3 "${router_root}/state.sqlite" <<'SQL'
INSERT INTO accounts (account_id, label, status, active_credential_generation)
VALUES
  ('acct_weak', 'weak', 'enabled', 1),
  ('acct_mid', 'mid', 'enabled', 1),
  ('acct_best', 'best', 'enabled', 1);

INSERT INTO selector_quota_windows (
  account_id,
  route_band,
  limit_window_seconds,
  status,
  remaining_headroom,
  reset_unix_seconds,
  effective,
  observed_unix_seconds
)
VALUES
  ('acct_weak', 'responses', 18000, 'eligible', 100, 17900, 1, 10000),
  ('acct_weak', 'responses', 604800, 'eligible', 23, 260000, 0, 10000),
  ('acct_mid', 'responses', 18000, 'eligible', 99, 16000, 1, 10000),
  ('acct_mid', 'responses', 604800, 'eligible', 34, 270000, 0, 10000),
  ('acct_best', 'responses', 18000, 'eligible', 89, 23500, 1, 10000),
  ('acct_best', 'responses', 604800, 'eligible', 77, 485000, 0, 10000);

INSERT INTO active_client_leases (
  route_band,
  process_run_id,
  reservation_id,
  account_id,
  acquired_unix_seconds,
  active_pressure
)
VALUES
  ('responses', 'process-smoke', 'reservation-weak-1', 'acct_weak', 9900, 2),
  ('responses', 'process-smoke', 'reservation-mid-1', 'acct_mid', 9900, 8);
SQL

json_output="${smoke_root}/quota-status.json"
stderr_output="${smoke_root}/quota-status.stderr"

"${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota status \
  --no-refresh \
  --router-root "${router_root}" \
  --format json \
  --now-unix-seconds 10000 \
  >"${json_output}" \
  2>"${stderr_output}"

grep -q "codex_router.process_start" "${stderr_output}"
grep -q "codex_router.quota_status_selection" "${stderr_output}"
grep -q 'active_client.source="sqlx_mirror"' "${stderr_output}"
grep -q "preferred.account_hash=" "${stderr_output}"
grep -q "plan1-observability-smoke" "${stderr_output}"

for forbidden in \
  "acct_weak" \
  "acct_mid" \
  "acct_best" \
  "reservation-weak-1" \
  "reservation-mid-1" \
  "access-token" \
  "refresh-token" \
  "authorization" \
  "X-Codex-Router-Token"; do
  if grep -q "${forbidden}" "${stderr_output}"; then
    echo "telemetry leaked forbidden text: ${forbidden}" >&2
    exit 1
  fi
done

for forbidden in \
  "acct_weak" \
  "acct_mid" \
  "acct_best" \
  "reservation-weak-1" \
  "reservation-mid-1" \
  "access-token" \
  "refresh-token" \
  "authorization" \
  "X-Codex-Router-Token"; do
  if grep -q "${forbidden}" "${json_output}"; then
    echo "json quota status leaked forbidden text: ${forbidden}" >&2
    exit 1
  fi
done

node verification/tooling/quota-smoke-assertions.ts plan1 "${json_output}"

echo "quota routing plan1 observability smoke ok: ${smoke_root}"
