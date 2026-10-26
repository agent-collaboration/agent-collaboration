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

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
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

collector_health_url="${CODEX_ROUTER_OTEL_COLLECTOR_HEALTH_URL:-http://127.0.0.1:13133/}"
metrics_url="${CODEX_ROUTER_VICTORIA_METRICS_URL:-http://127.0.0.1:8428}"
traces_url="${CODEX_ROUTER_VICTORIA_TRACES_URL:-http://127.0.0.1:10428}"

if ! curl --silent --show-error --fail --max-time 2 "${collector_health_url}" >/dev/null; then
  echo "local OTEL collector is not healthy at ${collector_health_url}" >&2
  echo "start the shared observability stack before running this smoke" >&2
  exit 2
fi

if ! curl --silent --show-error --fail --max-time 2 "${metrics_url}/api/v1/label/__name__/values" >/dev/null; then
  echo "VictoriaMetrics is not reachable at ${metrics_url}" >&2
  exit 2
fi

smoke_root="$(mktemp -d "${TMPDIR:-/tmp}/codex-router-plan3-otel.XXXXXX")"
router_root="${smoke_root}/router"
mkdir -p "${router_root}"

marker="codex-router-plan3-$(date +%s)-$$"
secret_canary="codex-router-secret-canary-${marker}"

export OTEL_EXPORTER_OTLP_ENDPOINT="${OTEL_EXPORTER_OTLP_ENDPOINT:-http://127.0.0.1:4318}"
export OTEL_EXPORTER_OTLP_PROTOCOL="${OTEL_EXPORTER_OTLP_PROTOCOL:-http/protobuf}"
export CODEX_ROUTER_OBSERVABILITY_MARKER="${marker}"
export CODEX_ROUTER_RUNTIME_FLAVOR="smoke"
export CODEX_ROUTER_RELEASE_CHANNEL="local"
export CODEX_ROUTER_FORBIDDEN_CANARY="${secret_canary}"
export RUST_LOG="warn,codex_router_cli=info,codex_router_proxy=info,opentelemetry_sdk=off,opentelemetry_otlp=off"

installed_codex_output="${smoke_root}/installed-codex-concurrent.out"
set +e
tests/smoke/installed_codex_mock.sh --transport websocket --scenario concurrent | tee "${installed_codex_output}"
installed_codex_status=${PIPESTATUS[0]}
set -e
if [[ "${installed_codex_status}" -ne 0 ]]; then
  exit "${installed_codex_status}"
fi

installed_codex_artifact="$(
  awk '/codex_router_three_websocket_artifact=/{sub(/^.*codex_router_three_websocket_artifact=/, ""); value=$0} END{print value}' "${installed_codex_output}"
)"
if [[ -z "${installed_codex_artifact}" || ! -f "${installed_codex_artifact}" ]]; then
  echo "installed-Codex concurrent WebSocket smoke did not produce an artifact path" >&2
  cat "${installed_codex_output}" >&2
  exit 1
fi

node verification/tooling/quota-smoke-assertions.ts plan3-installed "${installed_codex_artifact}"

"${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota \
  --no-refresh \
  --router-root "${router_root}" \
  --format plain \
  --now-unix-seconds 10000 \
  >/dev/null

sqlite3 "${router_root}/state.sqlite" <<'SQL'
INSERT INTO accounts (account_id, label, status, active_credential_generation)
VALUES
  ('acct_retire', 'askluna', 'enabled', 1),
  ('acct_reserve', 'matches', 'enabled', 1),
  ('acct_ssdev', 'ssdev', 'enabled', 1),
  ('acct_unknown', 'needsprobe', 'enabled', 1);

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
  ('acct_retire', 'responses', 18000, 'eligible', 98, 24400, 1, 10000),
  ('acct_retire', 'responses', 604800, 'eligible', 4, 269200, 0, 10000),
  ('acct_reserve', 'responses', 18000, 'eligible', 99, 26200, 1, 10000),
  ('acct_reserve', 'responses', 604800, 'eligible', 9, 272800, 0, 10000),
  ('acct_ssdev', 'responses', 18000, 'eligible', 78, 20800, 1, 10000),
  ('acct_ssdev', 'responses', 604800, 'eligible', 76, 485200, 0, 10000);

INSERT INTO active_client_leases (
  route_band,
  process_run_id,
  reservation_id,
  account_id,
  acquired_unix_seconds,
  active_pressure
)
VALUES
  ('responses', 'process-smoke', 'reservation-retire-1', 'acct_retire', 9900, 8),
  ('responses', 'process-smoke', 'reservation-reserve-1', 'acct_reserve', 9900, 8);
SQL

"${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota \
  --no-refresh \
  --router-root "${router_root}" \
  --format json \
  --now-unix-seconds 10000 \
  >"${smoke_root}/quota.json"

"${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota refresh \
  --router-root "${router_root}" \
  --base-url "https://chatgpt.com/backend-api" \
  >"${smoke_root}/quota-refresh.out" \
  2>"${smoke_root}/quota-refresh.err" || true

reject_root="${smoke_root}/reject-router"
mkdir -p "${reject_root}"
CODEX_ROUTER_OBSERVABILITY_MARKER="${marker}-setup" "${cargo_command[@]}" run -q -p codex-router-cli -- \
  quota \
  --no-refresh \
  --router-root "${reject_root}" \
  --format plain \
  --now-unix-seconds 10000 \
  >/dev/null

server_stdout="${smoke_root}/reject-server.stdout"
server_stderr="${smoke_root}/reject-server.stderr"
reject_port="$(
  node verification/tooling/smoke-network.ts free-port
)"
"${cargo_command[@]}" run -q -p codex-router-cli -- \
  serve \
  --port "${reject_port}" \
  --state-db "${reject_root}/state.sqlite" \
  --secret-root "${reject_root}/secrets" \
  --upstream-base-url "http://127.0.0.1:9/v1" \
  --disable-background-quota-refresh \
  --max-connections 2 \
  >"${server_stdout}" \
  2>"${server_stderr}" &
server_pid="$!"

cleanup_server() {
  if kill -0 "${server_pid}" >/dev/null 2>&1; then
    kill "${server_pid}" >/dev/null 2>&1 || true
    wait "${server_pid}" >/dev/null 2>&1 || true
  fi
}
trap cleanup_server EXIT

for _attempt in $(seq 1 50); do
  if grep -q "listening:" "${server_stdout}"; then
    break
  fi
  if ! kill -0 "${server_pid}" >/dev/null 2>&1; then
    echo "reject server exited before readiness" >&2
    cat "${server_stderr}" >&2
    exit 1
  fi
  sleep 0.1
done

if ! grep -q "listening:" "${server_stdout}"; then
  echo "reject server did not become ready" >&2
  cat "${server_stderr}" >&2
  exit 1
fi

listener="$(
  node verification/tooling/smoke-network.ts listener "${server_stdout}"
)"

reject_http_status="$(
  curl --silent --show-error --max-time 5 \
  --output "${smoke_root}/reject-http.out" \
  --write-out "%{http_code}" \
  --request POST \
  --header "content-type: application/json" \
  --data "{\"type\":\"response.create\",\"input\":\"${secret_canary}\"}" \
  "http://${listener}/v1/responses" || true
)"
if [[ "${reject_http_status}" != "503" ]]; then
  echo "expected no-account HTTP rejection status 503, got ${reject_http_status}" >&2
  cat "${smoke_root}/reject-http.out" >&2
  cat "${server_stderr}" >&2
  exit 1
fi

node verification/tooling/smoke-network.ts websocket "${listener}" "${secret_canary}" || true

for _attempt in $(seq 1 50); do
  if ! kill -0 "${server_pid}" >/dev/null 2>&1; then
    wait "${server_pid}" >/dev/null 2>&1 || true
    break
  fi
  sleep 0.1
done

if kill -0 "${server_pid}" >/dev/null 2>&1; then
  echo "reject server did not exit after bounded proof connections" >&2
  cat "${server_stdout}" >&2
  cat "${server_stderr}" >&2
  cleanup_server
  exit 1
fi
trap - EXIT

if ! grep -q "codex_router.account_selection_rejected" "${server_stderr}"; then
  echo "reject server did not log account selection rejection" >&2
  cat "${server_stderr}" >&2
  exit 1
fi

metric_series="${smoke_root}/metric-series.json"
trace_result="${smoke_root}/trace-result.json"

wait_for_metric() {
  local metric_selector="$1"
  local metric_query
  if [[ "${metric_selector}" == *"{"* ]]; then
    metric_query="${metric_selector%?},agent.proof.marker=\"${marker}\"}"
  else
    metric_query="${metric_selector}{agent.proof.marker=\"${marker}\"}"
  fi
  for _attempt in $(seq 1 20); do
    curl --silent --show-error --max-time 5 --get \
      "${metrics_url}/api/v1/series" \
      --data-urlencode "match[]=${metric_query}" \
      >"${metric_series}"
    if node verification/tooling/quota-smoke-assertions.ts metric-present "$metric_series"
    then
      return 0
    fi
    sleep 1
  done
  echo "metric ${metric_selector} was not exported with marker ${marker}" >&2
  cat "${metric_series}" >&2
  return 1
}

for metric_name in \
  codex_router_account_selections_total \
  codex_router_active_clients \
  codex_router_quota_refresh_total \
  codex_router_websocket_events_total \
  codex_router_quota_remaining_bucket \
  codex_router_quota_pressure_bucket; do
  wait_for_metric "${metric_name}"
done

wait_for_metric 'codex_router_account_rejections_total{selection.reason="no_eligible_accounts"}'
wait_for_metric 'codex_router_account_rejections_total{selection.reason="held_reserve"}'
wait_for_metric 'codex_router_account_rejections_total{selection.reason="held_unknown"}'
wait_for_metric 'codex_router_account_rejections_total{selection.reason="retiring_near_zero"}'
wait_for_metric 'codex_router_account_selections_total{selection.reason="preferred_weekly_healthier"}'
wait_for_metric 'codex_router_websocket_events_total{event.kind="open"}'

curl --silent --show-error --max-time 5 --get \
  "${metrics_url}/api/v1/series" \
  --data-urlencode "match[]={__name__=~\"codex_router_.*\",agent.proof.marker=\"${marker}\"}" \
  >"${metric_series}"

node verification/tooling/quota-smoke-assertions.ts metrics "${metric_series}" "${secret_canary}"

for _attempt in $(seq 1 20); do
  curl --silent --show-error --max-time 5 --get \
    "${traces_url}/select/logsql/query" \
    --data-urlencode "query=\"resource_attr:agent.proof.marker\":\"${marker}\"" \
    >"${trace_result}" || true
  if grep -q "codex_router.run" "${trace_result}"; then
    break
  fi
  sleep 1
done

for trace_name in \
  "codex_router.run" \
  "codex_router.websocket_open"; do
  if ! grep -q "${trace_name}" "${trace_result}"; then
    echo "VictoriaTraces did not contain ${trace_name} for marker ${marker}" >&2
    cat "${trace_result}" >&2
    exit 1
  fi
done

for forbidden in \
  "${secret_canary}" \
  "acct_retire" \
  "acct_reserve" \
  "acct_ssdev" \
  "acct_unknown" \
  "reservation-retire-1" \
  "reservation-reserve-1" \
  "access-token" \
  "refresh-token" \
  "authorization" \
  "X-Codex-Router-Token"; do
  if grep -q "${forbidden}" \
    "${trace_result}" \
    "${metric_series}" \
    "${installed_codex_artifact}" \
    "${server_stdout}" \
    "${server_stderr}" \
    "${smoke_root}/reject-http.out" \
    "${smoke_root}/quota-refresh.out" \
    "${smoke_root}/quota-refresh.err"; then
    echo "observability smoke leaked forbidden text: ${forbidden}" >&2
    exit 1
  fi
done

echo "quota routing plan3 observability smoke ok: marker=${marker} root=${smoke_root}"
