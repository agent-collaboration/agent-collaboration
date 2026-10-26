#!/usr/bin/env bash
set -euo pipefail

PLAN_ROOT="tmp/plan-workflows/2026-06-24-async-router-runtime"
EVIDENCE_ROOT="$PLAN_ROOT/evidence"
ACCOUNT_ROUTER_PLAN_ROOT="tmp/plan-workflows/2026-06-25-account-router-ux-fix"
ACCOUNT_ROUTER_EVIDENCE_ROOT="$ACCOUNT_ROUTER_PLAN_ROOT/evidence"

usage() {
  cat >&2 <<'EOF'
usage: scripts/proof-matrix.sh <ROW>

Runs one codex-router async-runtime proof-matrix row.
Pending rows intentionally exit non-zero until their real harness exists.
EOF
}

json_escape() {
  local value="$1"
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\n'/\\n}
  value=${value//$'\r'/\\r}
  value=${value//$'\t'/\\t}
  printf '%s' "$value"
}

row_layer() {
  case "$1" in
    AR-WS-GUARD) printf 'structural' ;;
    AR-E2E-ACCOUNTS) printf 'e2e' ;;
    AR-*) printf 'integration' ;;
    U-*) printf 'unit' ;;
    I-*) printf 'integration' ;;
    S-*) printf 'smoke' ;;
    E-*) printf 'e2e' ;;
    G-*) printf 'structural' ;;
    P-*) printf 'pr-gate' ;;
    *) return 1 ;;
  esac
}

row_owner() {
  case "$1" in
    AR-*) printf 'account-router-ux-fix' ;;
    U-06|U-07|I-16) printf 'T2' ;;
    I-21) printf 'T1/T2' ;;
    I-19) printf 'T5' ;;
    I-20) printf 'T4/T5' ;;
    I-17b) printf 'T5/T6' ;;
    S-01|S-02|S-03|S-04) printf 'T7' ;;
    E-01|E-02|E-03|E-04|E-05|E-06|E-07|E-08|E-09) printf 'T8' ;;
    G-01|G-02|G-03|G-04|G-05|G-06|G-07|G-08|G-09|G-10|G-11|G-12|G-13|G-14|G-15|G-16|G-17|G-18|G-19|G-20|G-21|G-22|G-23|G-24|G-25|G-26|G-27|G-28|G-29) printf 'T6' ;;
    P-01|P-02|P-03|P-04|P-05|P-06|P-09) printf 'T0/T6' ;;
    P-07|P-08|P-10) printf 'final' ;;
    *) printf 'pending-owner' ;;
  esac
}

expected_observation() {
  case "$1" in
    AR-WS-GUARD)
      printf 'release WebSocket routing has no first-frame size cap, first-frame type rejection, or whole-frame JSON validity gate'
      ;;
    AR-WS-PASSTHROUGH)
      printf 'real serve path forwards a legal >1 MiB first WebSocket data frame byte-identically'
      ;;
    AR-WS-ACTIVE-TURN)
      printf 'same WebSocket re-reserves active turn load on later request-like frames for the pinned account'
      ;;
    AR-HTTP-PASSTHROUGH)
      printf 'HTTP/SSE request bodies and response streams remain pass-through while bounded metadata is observed'
      ;;
    AR-CLI-UX)
      printf 'normal CLI help/default command contract hides internal proof/import/token noise'
      ;;
    AR-SESSIONS)
      printf 'sessions defaults to cwd, supports checkout/repo/any root flags, and reads Codex state read-only'
      ;;
    AR-SCHEMA)
      printf 'default router root and SQLite schema behavior are reproducible for fresh and drifted databases'
      ;;
    AR-E2E-ACCOUNTS)
      printf 'three concurrent installed Codex clients prove WebSocket continuity and selected account tags'
      ;;
    E-02)
      printf 'five-minute soak holds three installed Codex WebSocket sessions through one real router process'
      ;;
    E-01)
      printf 'three installed Codex WebSocket children share one router PID and complete without fallback/retry/downgrade'
      ;;
    E-03)
      printf 'soak artifact overlap timestamps prove concurrent activity'
      ;;
    E-04)
      printf 'router writes at least three upstream frames to each of three completed sessions during overlap'
      ;;
    E-05)
      printf 'one installed Codex runtime completes function-call-style multi-step WebSocket interleave during overlap'
      ;;
    E-06)
      printf 'router-owned WebSocket registry records high-water 3 and zero active after completion'
      ;;
    E-07)
      printf 'three client processes share one router PID with aggregate unique router/upstream WebSocket session continuity and clean client transports'
      ;;
    E-08)
      printf 'live router socket table has no leaked ESTABLISHED or CLOSE_WAIT TCP sessions after completion'
      ;;
    E-09)
      printf 'three-WebSocket artifact allowlist redaction passes and negative canaries are absent'
      ;;
    S-01)
      printf 'installed Codex tokenless default router profile succeeds without CODEX_ROUTER_TOKEN'
      ;;
    S-02)
      printf 'installed Codex hardening smoke rejects missing/wrong/smuggled/stale local auth before upstream selection'
      ;;
    S-03)
      printf 'installed Codex smoke uses spawned built codex-router serve binary and records process metadata'
      ;;
    S-04)
      printf 'installed Codex mock upstream proves deterministic WebSocket behavior without fallback/reconnect/retry'
      ;;
    I-21)
      printf 'listener binds and first request is accepted while broad quota refresh is slow or stalled'
      ;;
    I-05a)
      printf 'old single-lane WebSocket reproducer is represented by a blocked-first-WebSocket fixture'
      ;;
    I-05b)
      printf 'async runtime lets survivor WebSocket complete and drains active sessions cleanly'
      ;;
    I-17b)
      printf 'slow affinity recorder cannot delay WebSocket frame forwarding or close progress'
      ;;
    I-18)
      printf 'served-router WebSocket fixture traverses listener, Hyper upgrade, registry cleanup, and blocked background refresh'
      ;;
    I-19)
      printf 'local/upstream close, runtime shutdown, and active pump cancellation clean up WebSocket sessions'
      ;;
    I-20)
      printf 'first client WebSocket frame is forwarded unchanged before upstream response pumping'
      ;;
    G-01)
      printf 'no production std::net listener or stream in release serve path'
      ;;
    G-02)
      printf 'no production reqwest::blocking in release serve HTTP upstream'
      ;;
    G-03)
      printf 'no blocking tungstenite accept/connect in release serve path'
      ;;
    G-04)
      printf 'no production httparse serving or upstream response parsing'
      ;;
    G-05)
      printf 'no blocking Read response body in release async runtime files'
      ;;
    G-06)
      printf 'no direct proxy runtime rusqlite access in release serve path'
      ;;
    G-07)
      printf 'positive Hyper and tokio-tungstenite ownership in release runtime'
      ;;
    G-08)
      printf 'no helper/private alternate parser, handshake, or frame runtime is reachable from release serve'
      ;;
    G-09)
      printf 'release reachability checker covers non-test production serve path and excludes only unreachable fixtures'
      ;;
    G-10)
      printf 'tokio-tungstenite is the production WebSocket protocol dependency; blocking tungstenite remains test/dev-only'
      ;;
    G-11)
      printf 'legacy blocking runtime is test-only and there is exactly one production serve runtime path'
      ;;
    G-12)
      printf 'WebSocket pumps use bounded side effects and persistence cannot delay forwarding or close'
      ;;
    G-13)
      printf 'compound close-while-pending regression remains in the permanent suite'
      ;;
    G-14)
      printf 'same-session WebSocket interleave regression remains in the permanent suite'
      ;;
    G-15)
      printf 'blocked-write/backpressure cleanup regression remains in the permanent suite'
      ;;
    G-16)
      printf 'mixed WebSocket plus HTTP/SSE progress regression remains in the permanent suite'
      ;;
    G-17)
      printf 'installed-Codex concurrent WebSocket smoke/e2e remains in the permanent suite'
      ;;
    G-18)
      printf 'long-running three-runtime soak remains available'
      ;;
    G-19)
      printf 'real serve close-while-pending regression remains in the permanent suite'
      ;;
    G-20)
      printf 'pump-side side-effect non-blocking regression remains in the permanent suite'
      ;;
    G-21)
      printf 'release runtime structural guardrails run through proof-matrix'
      ;;
    G-22)
      printf 'no unbounded production pump channels and no detached production reader tasks in release serve path'
      ;;
    G-23)
      printf 'local Hyper websocket upgrade handoff has no double handshake'
      ;;
    G-24)
      printf 'HTTP/SSE release path streams request bodies without full collection before upstream egress'
      ;;
    G-25)
      printf 'HTTP/SSE affinity extraction is bounded and WebSocket metadata side effects do not gate upstream frame forwarding'
      ;;
    G-26)
      printf 'supported application routes are upstream pass-through and /v1/models is not synthesized'
      ;;
    G-27)
      printf 'WebSocket pre-upstream parsing is metadata-only and does not require prompt-bearing payload shape'
      ;;
    G-28)
      printf 'release serve path has no WebSocket message-count truncation, first-frame timeout, provider-event-aware termination, or router-created retry/fallback policy'
      ;;
    G-29)
      printf 'release serve request-time auth/state boundary hides secret-store, refresh client, raw SQL, and credential generation commit internals from proxy serve paths'
      ;;
    *)
      printf 'row harness has not been implemented yet'
      ;;
  esac
}

freshness_guard() {
  printf 'verification/tooling/*.ts verification/tooling/proof-receipt-patches.json verification/fixtures/*.ts verification/package.json verification/pnpm-lock.yaml verification/evidence-json.ts verification/exact-json.ts verification/filesystem-paths.ts verification/fixtures/source-shell-words.ts '
  case "$1" in
    AR-WS-GUARD) printf 'crates/codex-router-proxy/src/websocket.rs crates/codex-router-proxy/src/server.rs verification/tooling/check-release-runtime-guardrails.ts scripts/proof-matrix.sh' ;;
    AR-WS-PASSTHROUGH|AR-WS-ACTIVE-TURN) printf 'crates/codex-router-proxy/src/websocket.rs crates/codex-router-proxy/src/server.rs crates/codex-router-proxy/src/lib.rs crates/codex-router-selection/src/* scripts/proof-matrix.sh' ;;
    AR-HTTP-PASSTHROUGH) printf 'crates/codex-router-proxy/src/http_sse.rs crates/codex-router-proxy/src/upstream.rs crates/codex-router-proxy/src/server.rs crates/codex-router-proxy/src/lib.rs scripts/proof-matrix.sh' ;;
    AR-CLI-UX) printf 'crates/codex-router-cli/src/* Cargo.toml crates/codex-router-cli/Cargo.toml scripts/proof-matrix.sh' ;;
    AR-SESSIONS) printf 'crates/codex-router-cli/src/sessions.rs crates/codex-router-cli/src/lib.rs crates/codex-router-state/src/* scripts/proof-matrix.sh' ;;
    AR-SCHEMA) printf 'crates/codex-router-state/src/* crates/codex-router-cli/src/* scripts/proof-matrix.sh' ;;
    AR-E2E-ACCOUNTS) printf 'crates/codex-router-test-support/src/* tests/smoke/* crates/codex-router-proxy/src/* crates/codex-router-cli/src/* scripts/proof-matrix.sh' ;;
    I-19|I-20) printf 'crates/codex-router-proxy/src/websocket.rs crates/codex-router-proxy/src/server.rs crates/codex-router-proxy/src/lib.rs scripts/proof-matrix.sh' ;;
    I-21) printf 'crates/codex-router-cli/src/lib.rs crates/codex-router-proxy/src/server.rs crates/codex-router-quota/src/* crates/codex-router-state/src/*' ;;
    U-*) printf 'crates/codex-router-proxy/src/* crates/codex-router-core/src/* crates/codex-router-selection/src/*' ;;
    I-*) printf 'crates/codex-router-proxy/src/* crates/codex-router-cli/src/* crates/codex-router-state/src/* crates/codex-router-auth/src/*' ;;
    S-*|E-*) printf 'crates/codex-router-test-support/src/* tests/smoke/* crates/codex-router-cli/src/* crates/codex-router-proxy/src/* scripts/proof-matrix.sh' ;;
    G-*) printf 'Cargo.toml crates/*/Cargo.toml crates/codex-router-proxy/src/* scripts/* .github/workflows/*' ;;
    P-*) printf 'tmp/plan-workflows/2026-06-24-async-router-runtime/* tmp/spec-workflows/2026-06-24-async-router-runtime/*' ;;
  esac
}

ensure_guarded_source_paths_clean() {
  local row_id="$1"
  local guard
  guard="$(freshness_guard "$row_id")"
  node verification/tooling/proof-matrix-receipt.ts guard "$row_id" "$guard"
}

known_row() {
  case "$1" in
    AR-WS-GUARD|AR-WS-PASSTHROUGH|AR-WS-ACTIVE-TURN|AR-HTTP-PASSTHROUGH|AR-CLI-UX|AR-SESSIONS|AR-SCHEMA|AR-E2E-ACCOUNTS) return 0 ;;
    U-01|U-02|U-03|U-04|U-05|U-06|U-07) return 0 ;;
    I-01|I-02|I-03|I-04|I-05a|I-05b|I-06|I-07|I-08|I-09|I-10|I-11|I-12|I-13|I-14|I-15|I-16|I-17a|I-17b|I-18|I-19|I-20|I-21) return 0 ;;
    S-01|S-02|S-03|S-04) return 0 ;;
    E-01|E-02|E-03|E-04|E-05|E-06|E-07|E-08|E-09) return 0 ;;
    G-01|G-02|G-03|G-04|G-05|G-06|G-07|G-08|G-09|G-10|G-11|G-12|G-13|G-14|G-15|G-16|G-17|G-18|G-19|G-20|G-21|G-22|G-23|G-24|G-25|G-26|G-27|G-28|G-29) return 0 ;;
    P-01|P-02|P-03|P-04|P-05|P-06|P-07|P-08|P-09|P-10) return 0 ;;
    *) return 1 ;;
  esac
}

write_receipt() {
  local row_id="$1"
  local layer="$2"
  local owner="$3"
  local result="$4"
  local exit_code="$5"
  local root="$EVIDENCE_ROOT"
  case "$row_id" in
    AR-*) root="$ACCOUNT_ROUTER_EVIDENCE_ROOT" ;;
  esac
  local evidence_dir="$root/$layer"
  local timestamp
  local git_head
  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  git_head=$(git rev-parse HEAD)
  mkdir -p "$evidence_dir"
  local receipt="$evidence_dir/$row_id.json"
  cat > "$receipt" <<EOF
{
  "schema_version": 1,
  "row_id": "$(json_escape "$row_id")",
  "layer": "$(json_escape "$layer")",
  "owner": "$(json_escape "$owner")",
  "command": "scripts/proof-matrix.sh $(json_escape "$row_id")",
  "cwd": ".",
  "git_head": "$(json_escape "$git_head")",
  "timestamp_utc": "$(json_escape "$timestamp")",
  "status_before": "[ ] pending",
  "status_after": "[ ] pending",
  "result": "$(json_escape "$result")",
  "exit_code": $exit_code,
  "expected_observation": "$(json_escape "$(expected_observation "$row_id")")",
  "touched_targets": [],
  "freshness_guard": "$(json_escape "$(freshness_guard "$row_id")")",
  "freshness_check": "not_evaluated",
  "redaction_check": "pass",
  "artifact_paths": [],
  "notes": "T1 scaffold receipt; pending rows are not proof of implementation."
}
EOF
  printf '%s\n' "$receipt"
}

write_verify_only_receipt() {
  local row_id="$1"
  local layer="$2"
  local owner="$3"
  local result="$4"
  local exit_code="$5"
  local timestamp
  local git_head
  local receipt
  timestamp=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  git_head=$(git rev-parse HEAD)
  receipt=$(mktemp "${TMPDIR:-/tmp}/codex-router-proof-${row_id}.XXXXXX")
  cat > "$receipt" <<EOF
{
  "schema_version": 1,
  "row_id": "$(json_escape "$row_id")",
  "layer": "$(json_escape "$layer")",
  "owner": "$(json_escape "$owner")",
  "command": "scripts/proof-matrix.sh $(json_escape "$row_id")",
  "cwd": ".",
  "git_head": "$(json_escape "$git_head")",
  "timestamp_utc": "$(json_escape "$timestamp")",
  "status_before": "[ ] pending",
  "status_after": "[ ] pending",
  "result": "$(json_escape "$result")",
  "exit_code": $exit_code,
  "expected_observation": "$(json_escape "$(expected_observation "$row_id")")",
  "touched_targets": [],
  "freshness_guard": "$(json_escape "$(freshness_guard "$row_id")")",
  "freshness_check": "verify_only_not_persisted",
  "redaction_check": "pass",
  "artifact_paths": [],
  "notes": "verify-only receipt; not persisted to repo evidence."
}
EOF
  printf '%s\n' "$receipt"
}

write_proof_receipt() {
  if [[ "${CODEX_ROUTER_PROOF_VERIFY_ONLY:-}" == "1" ]]; then
    write_verify_only_receipt "$@"
  else
    write_receipt "$@"
  fi
}

mark_row_pass() {
  local row_id="$1"
  local layer="$2"
  local owner="$3"
  local notes="$4"
  shift 4

  for command in "$@"; do
    bash -lc "$command"
  done

  if ! ensure_guarded_source_paths_clean "$row_id"; then
    receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
    printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
    exit 1
  fi

  receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
  node verification/tooling/proof-matrix-receipt.ts pass "$receipt" "$notes"
  printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
  exit 0
}

main() {
  if [[ $# -ne 1 ]]; then
    usage
    exit 2
  fi

  local row_id="$1"
  if ! known_row "$row_id"; then
    printf 'unknown proof row: %s\n' "$row_id" >&2
    usage
    exit 2
  fi

  local layer
  layer=$(row_layer "$row_id")
  local owner
  owner=$(row_owner "$row_id")

  case "$row_id" in
    AR-WS-GUARD)
      CODEX_ROUTER_PROOF_COMMAND="scripts/proof-matrix.sh $row_id" \
        node verification/tooling/check-release-runtime-guardrails.ts "$row_id"
      exit $?
      ;;
    AR-WS-PASSTHROUGH)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "WebSocket first-frame pass-through tests prove large/malformed/future Codex-owned frames are forwarded without router payload policy." \
        "cargo test -p codex-router-proxy loopback_router_runtime_passes_large_malformed_websocket_first_frame_unchanged -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_first_frame_forwards_large_malformed_payload_without_router_policy -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_first_response_create_frame_selects_and_forwards_unchanged -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_first_future_json_payload_selects_and_forwards_unchanged -- --nocapture"
      ;;
    AR-WS-ACTIVE-TURN)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "WebSocket active-turn reservation tests prove completion releases load and later same-socket request-like frames re-reserve the pinned account." \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::turn_reservation_tests::response_completed_releases_active_reservation_before_socket_closes -- --nocapture" \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::metadata_boundary_tests::completion_releases_before_blocked_affinity_recording -- --nocapture" \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::turn_reservation_tests::same_socket_request_after_completion_reserves_pinned_account_again -- --nocapture"
      ;;
    AR-HTTP-PASSTHROUGH)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "HTTP/SSE pass-through tests prove request bodies stream before EOF and response streams are forwarded while bounded metadata is observed." \
        "cargo test -p codex-router-proxy upstream::hyper_http_upstream_transport_streams_request_body_before_eof -- --nocapture" \
        "cargo test -p codex-router-proxy assembled_loopback_router_runtime_streams_sse_before_upstream_eof -- --nocapture" \
        "cargo test -p codex-router-proxy http_proxy_preserves_responses_body_bytes_without_interpreting_unknown_fields -- --nocapture"
      ;;
    AR-CLI-UX)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "CLI UX tests and help smoke prove normal help/defaults hide internal token/import/live/proof noise and expose installed-app commands." \
        "cargo test -p codex-router-cli process_binary_path_is_skipped_before_command_parse -- --nocapture" \
        "cargo test -p codex-router-cli account_login_defaults_to_device_auth_method -- --nocapture" \
        "cargo test -p codex-router-cli account_list_renders_friendly_table_without_account_ids -- --nocapture" \
        "cargo test -p codex-router-cli quota_command_defaults_to_status -- --nocapture" \
        "cargo run -q -p codex-router-cli -- --help | grep -F 'sessions --repo' >/dev/null" \
        "! cargo run -q -p codex-router-cli -- --help | grep -E -- '--state-db|--secret-root|import-codex-auth|live quota|--scope|--allow-plaintext-file-secrets' >/dev/null"
      ;;
    AR-SESSIONS)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Sessions tests prove cwd default, checkout/repo/any root flags, provider/source filters, picker launch, and SQLx read-only state boundary." \
        "cargo test -p codex-router-cli sessions_ -- --nocapture"
      ;;
    AR-SCHEMA)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "State/default-root tests prove current schema migrations, reset-credit migration, unsupported schema fail-closed behavior, and ~/.codex-router defaults." \
        "cargo test -p codex-router-state sqlite_migration_roundtrips_account_and_quota_snapshot -- --nocapture" \
        "cargo test -p codex-router-state v6_migration_adds_reset_credits_without_losing_existing_quota -- --nocapture" \
        "cargo test -p codex-router-state unsupported_schema_version_fails_closed_on_open -- --nocapture" \
        "cargo test -p codex-router-cli serve_command_defaults_to_home_router_paths_and_provider_upstream -- --nocapture" \
        "cargo test -p codex-router-cli account_list_command_defaults_to_home_router_root -- --nocapture" \
        "cargo test -p codex-router-cli quota_status_command_defaults_to_home_router_root -- --nocapture"
      ;;
    AR-E2E-ACCOUNTS)
      serial_output_file=$(mktemp "${TMPDIR:-/tmp}/codex-router-${row_id}-serial.XXXXXX")
      concurrent_output_file=$(mktemp "${TMPDIR:-/tmp}/codex-router-${row_id}-concurrent.XXXXXX")
      if ! tests/smoke/installed_codex_mock.sh --transport websocket --scenario serial 2>&1 | tee "$serial_output_file"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; serial installed-Codex smoke failed; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      if ! tests/smoke/installed_codex_mock.sh --transport websocket --scenario concurrent 2>&1 | tee "$concurrent_output_file"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; concurrent installed-Codex smoke failed; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      serial_artifact=$(awk '/codex_router_installed_codex_artifact=/{sub(/^.*codex_router_installed_codex_artifact=/, ""); value=$0} END{print value}' "$serial_output_file")
      concurrent_artifact=$(awk '/codex_router_three_websocket_artifact=/{sub(/^.*codex_router_three_websocket_artifact=/, ""); value=$0} END{print value}' "$concurrent_output_file")
      if [[ -z "$serial_artifact" || ! -f "$serial_artifact" || -z "$concurrent_artifact" || ! -f "$concurrent_artifact" ]]; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; smoke did not emit durable transcript artifacts; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      if ! ensure_guarded_source_paths_clean "$row_id"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
      if node verification/tooling/proof-matrix-receipt.ts accounts "$receipt" "$serial_artifact" "$concurrent_artifact"
      then
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    U-01)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Route classification and unsupported route unit proofs passed." \
        "cargo test -p codex-router-proxy route_classifier_supports_required_codex_routes_and_rejects_realtime -- --nocapture" \
        "cargo test -p codex-router-proxy http_proxy_rejects_unsupported_paths_before_upstream -- --nocapture" \
        "cargo test -p codex-router-proxy loopback_router_runtime_rejects_unsupported_websocket_path_before_accept -- --nocapture"
      ;;
    U-02)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Local auth tokenless/token-required and smuggling unit proofs passed." \
        "cargo test -p codex-router-proxy local_auth -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_http_proxy_rejects_forbidden_local_auth_carriers_before_selection -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_websocket_router_rejects_first_frame_auth_smuggling_before_selection -- --nocapture"
      ;;
    U-03)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "WebSocket first request data validation and pre-duplex cancellation proofs passed." \
        "cargo test -p codex-router-proxy websocket_first_frame_rejects_hostile_preselection_cases -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_websocket_router_rejects_first_frame_before_selection_or_credentials -- --nocapture" \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::handshake_outcome_tests::runtime_shutdown_cancels_pending_first_frame_routing -- --nocapture"
      ;;
    U-04)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "HTTP/SSE and WebSocket affinity metadata extraction unit proofs passed." \
        "cargo test -p codex-router-proxy authenticated_http_proxy_records_top_level_response_id_owner -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_http_proxy_ignores_nested_response_ids -- --nocapture" \
        "cargo test -p codex-router-proxy async_websocket_tunnel_records_top_level_response_owner -- --nocapture"
      ;;
    U-05)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Header sanitation proofs passed for HTTP/SSE and WebSocket upstream auth injection." \
        "cargo test -p codex-router-proxy upstream_request_strips_local_and_hop_headers_and_injects_auth_once -- --nocapture" \
        "cargo test -p codex-router-proxy async_websocket_tunnel_sanitizes_upstream_handshake -- --nocapture"
      ;;
    U-06)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Selection preservation proofs passed across async state projection and proxy selector behavior." \
        "cargo test -p codex-router-state async_selector_input_matches_sync_repository_projection -- --nocapture" \
        "cargo test -p codex-router-proxy async_repository_backed_selector -- --nocapture" \
        "cargo test -p codex-router-proxy repository_backed_selector -- --nocapture"
      ;;
    U-08)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Release request-time async trait compile and selector boundary proofs passed." \
        "cargo check -p codex-router-proxy" \
        "cargo test -p codex-router-proxy async_repository_backed_selector -- --nocapture"
      ;;
    I-01)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Concurrent WebSocket sessions progress independently in the mock runtime fixture." \
        "cargo test -p codex-router-proxy loopback_router_runtime_accepts_second_websocket_while_first_is_blocked -- --nocapture"
      ;;
    I-02)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "A stalled upstream WebSocket does not block sibling WebSocket completion." \
        "cargo test -p codex-router-proxy loopback_router_runtime_accepts_second_websocket_while_first_is_blocked -- --nocapture"
      ;;
    I-03)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "A stalled WebSocket does not block independent HTTP/SSE completion." \
        "cargo test -p codex-router-proxy loopback_router_runtime_accepts_http_while_websocket_is_blocked -- --nocapture"
      ;;
    I-04)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Same-session bidirectional interleave proof passed before response completion." \
        "cargo test -p codex-router-proxy async_websocket_tunnel_forwards_first_frame_and_second_local_frame -- --nocapture"
      ;;
    I-06)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Upstream/local close cleanup proof passed through async pump shutdown fixtures." \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::shutdown_floor_tests::runtime_shutdown_cancels_active_duplex_pumps -- --nocapture" \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::idle_close_tests::reset_after_idle_control_frame_remains_clean_after_completion -- --nocapture"
      ;;
    I-07)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Blocked write/backpressure cleanup proof passed through duplex pump cancellation fixture." \
        "cargo test -p codex-router-proxy websocket::forwarding_tests::shutdown_floor_tests::runtime_shutdown_cancels_active_duplex_pumps -- --nocapture"
      ;;
    I-08)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Post-upgrade pre-upstream failure classes close deterministically with redacted router outcomes." \
        "cargo test -p codex-router-proxy authenticated_websocket_router_rejects_first_frame_before_selection_or_credentials -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_websocket_router_requires_affinity_secret_before_selection -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_websocket_router_missing_refresh_token_fails_closed_before_upstream_open -- --nocapture"
      ;;
    I-09)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Fragmented WebSocket upgrade proof passed through the loopback runtime." \
        "cargo test -p codex-router-proxy loopback_router_runtime_accepts_fragmented_websocket_upgrade -- --nocapture"
      ;;
    I-10)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Unsupported HTTP and WebSocket routes reject before selection/upstream." \
        "cargo test -p codex-router-proxy http_proxy_rejects_unsupported_paths_before_upstream -- --nocapture" \
        "cargo test -p codex-router-proxy loopback_router_runtime_rejects_unsupported_websocket_path_before_accept -- --nocapture"
      ;;
    I-11)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Hostile auth probes reject before route classification, selection, or upstream egress." \
        "cargo test -p codex-router-proxy authenticated_http_proxy_rejects_forbidden_local_auth_carriers_before_selection -- --nocapture" \
        "cargo test -p codex-router-proxy loopback_router_runtime_rejects_websocket_subprotocol_token_smuggling_before_accept -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_websocket_router_rejects_first_frame_auth_smuggling_before_selection -- --nocapture"
      ;;
    I-12)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Local router auth stripping and upstream auth injection canaries passed." \
        "cargo test -p codex-router-proxy upstream_request_strips_local_and_hop_headers_and_injects_auth_once -- --nocapture" \
        "cargo test -p codex-router-test-support route_native_black_box_all_supported_routes_and_rejections -- --ignored --nocapture"
      ;;
    I-13)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Previous-response affinity owner recording proofs passed for HTTP/SSE and WebSocket." \
        "cargo test -p codex-router-proxy authenticated_http_proxy_records_top_level_response_id_owner -- --nocapture" \
        "cargo test -p codex-router-proxy authenticated_http_proxy_records_streaming_sse_response_id_after_body_read -- --nocapture" \
        "cargo test -p codex-router-proxy async_websocket_tunnel_records_top_level_response_owner -- --nocapture"
      ;;
    I-14)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Token generation revocation closes stale explicit-token WebSocket sessions only." \
        "cargo test -p codex-router-proxy loopback_router_runtime_reloads_local_auth_and_closes_old_token_websocket -- --nocapture" \
        "cargo test -p codex-router-proxy websocket::registry_tests::registry_revokes_only_stale_generation_cancellations -- --nocapture"
      ;;
    I-15)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Registry cleanup proof passed for close/revocation/shutdown paths." \
        "cargo test -p codex-router-proxy websocket::registry_tests::registry_snapshot_tracks_active_high_water_and_cleanup -- --nocapture" \
        "cargo test -p codex-router-proxy loopback_router_runtime_shutdown_drains_active_websocket_sessions -- --nocapture"
      ;;
    E-01|E-02|E-03|E-04|E-05|E-06|E-07|E-08|E-09)
      three_websocket_artifact="${CODEX_ROUTER_THREE_WEBSOCKET_ARTIFACT:-}"
      artifact_source="CODEX_ROUTER_THREE_WEBSOCKET_ARTIFACT"
      if [[ -z "$three_websocket_artifact" ]]; then
        three_websocket_artifact_pointer="${CODEX_ROUTER_THREE_WEBSOCKET_ARTIFACT_POINTER:-tmp/smoke/installed-codex-three-websocket-soak-artifact.txt}"
        artifact_source="$three_websocket_artifact_pointer"
        if [[ -f "$three_websocket_artifact_pointer" ]]; then
          three_websocket_artifact="$(<"$three_websocket_artifact_pointer")"
        fi
      fi
      if [[ -z "$three_websocket_artifact" ]]; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; no explicit installed-codex three-websocket artifact found; run tests/smoke/installed_codex_mock.sh --transport websocket --scenario soak first or set CODEX_ROUTER_THREE_WEBSOCKET_ARTIFACT; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      if [[ ! -f "$three_websocket_artifact" ]]; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; installed-codex three-websocket artifact does not exist: %s; receipt: %s\n' "$row_id" "$three_websocket_artifact" "$receipt" >&2
        exit 1
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
      if node verification/tooling/proof-matrix-receipt.ts soak "$row_id" "$three_websocket_artifact" "$receipt" "$artifact_source" "$(freshness_guard "$row_id")"
      then
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    S-01|S-03|S-04)
      smoke_output_file=$(mktemp "${TMPDIR:-/tmp}/codex-router-${row_id}-smoke.XXXXXX")
      if tests/smoke/installed_codex_mock.sh --transport websocket --scenario serial 2>&1 | tee "$smoke_output_file"; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        artifact_path=$(awk '/codex_router_installed_codex_artifact=/{sub(/^.*codex_router_installed_codex_artifact=/, ""); value=$0} END{print value}' "$smoke_output_file")
        if [[ -z "$artifact_path" || ! -f "$artifact_path" ]]; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; serial installed-Codex smoke did not emit a durable artifact path; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts serial "$row_id" "$receipt" "$artifact_path" "$smoke_output_file"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    S-02)
      if cargo test -p codex-router-test-support installed_codex_hostile_no_token_smoke_keeps_upstream_empty -- --ignored --nocapture --test-threads=1 \
        && cargo test -p codex-router-proxy authenticated_http_proxy_rejects_forbidden_local_auth_carriers_before_selection -- --nocapture \
        && cargo test -p codex-router-proxy loopback_router_runtime_reloads_local_auth_and_closes_old_token_websocket -- --nocapture \
        && cargo test -p codex-router-proxy loopback_router_runtime_rejects_websocket_subprotocol_token_smuggling_before_accept -- --nocapture \
        && cargo test -p codex-router-proxy authenticated_websocket_router_rejects_first_frame_auth_smuggling_before_selection -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "S-02"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-17b)
      if cargo test -p codex-router-proxy async_websocket_tunnel_does_not_gate_forwarding_on_slow_affinity_recorder -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-17b"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-05a)
      if cargo test -p codex-router-proxy legacy_single_lane_accept_loop_reproducer_blocks_second_client_until_first_finishes -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-05a"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-05b)
      if cargo test -p codex-router-proxy loopback_router_runtime_accepts_second_websocket_while_first_is_blocked -- --nocapture \
        && cargo test -p codex-router-proxy loopback_router_runtime_shutdown_drains_active_websocket_sessions -- --nocapture \
        && cargo test -p codex-router-proxy runtime_shutdown_cancels_active_duplex_pumps -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-05b"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-18)
      if cargo test -p codex-router-cli served_router_websocket_uses_persisted_quota_while_background_refresh_is_blocked -- --nocapture \
        && cargo test -p codex-router-proxy loopback_router_runtime_shutdown_drains_active_websocket_sessions -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-18"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-19)
      if cargo test -p codex-router-proxy runtime_shutdown_cancels_active_duplex_pumps -- --nocapture \
        && cargo test -p codex-router-proxy runtime_shutdown_cancels_pending_first_frame_routing -- --nocapture \
        && cargo test -p codex-router-proxy runtime_shutdown_cancels_pending_upstream_connect -- --nocapture \
        && cargo test -p codex-router-proxy loopback_router_runtime_shutdown_drains_active_websocket_sessions -- --nocapture \
        && cargo test -p codex-router-proxy websocket::forwarding_tests::idle_close_tests::reset_during_new_turn_after_prior_completion_is_clean_transport_close -- --nocapture \
        && cargo test -p codex-router-proxy websocket::forwarding_tests::idle_close_tests::reset_after_idle_control_frame_remains_clean_after_completion -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-19"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-20)
      if cargo test -p codex-router-proxy websocket_first_response_create_frame_selects_and_forwards_unchanged -- --nocapture \
        && cargo test -p codex-router-proxy websocket_first_direct_response_create_payload_selects_and_forwards_unchanged -- --nocapture \
        && cargo test -p codex-router-proxy async_websocket_tunnel_forwards_first_frame_and_second_local_frame -- --nocapture; then
        if ! ensure_guarded_source_paths_clean "$row_id"; then
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
          printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
          exit 1
        fi
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
        node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-20"
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    I-21)
      if node verification/tooling/proof-matrix-receipt.ts http
      then
        if cargo test -p codex-router-cli served_router_http_uses_persisted_quota_while_background_refresh_is_blocked -- --nocapture; then
          if ! ensure_guarded_source_paths_clean "$row_id"; then
            receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
            printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
            exit 1
          fi
          receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
          node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "I-21"
          printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
          exit 0
        fi
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    G-06|G-08|G-09|G-10|G-11|G-12|G-13|G-14|G-15|G-16|G-17|G-18|G-19|G-20|G-22|G-29)
      if ! ensure_guarded_source_paths_clean "$row_id"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
      if node verification/tooling/proof-matrix-receipt.ts structural "$row_id" "$receipt"
      then
        printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
        exit 0
      fi
      printf 'proof row %s failed; receipt: %s\n' "$row_id" "$receipt" >&2
      exit 1
      ;;
    G-01|G-02|G-03|G-04|G-05|G-07|G-23|G-28)
      CODEX_ROUTER_PROOF_COMMAND="scripts/proof-matrix.sh $row_id" \
        node verification/tooling/check-release-runtime-guardrails.ts "$row_id"
      exit $?
      ;;
    G-24)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Structural guard and streaming behavior passed: release HTTP/SSE request bodies are forwarded as Hyper bodies without full collection before upstream egress, and routing metadata survives split DATA frames." \
        "node verification/tooling/check-http-streaming-guard.ts G-24" \
        "cargo test -p codex-router-proxy hyper_http_upstream_transport_streams_request_body_before_eof -- --nocapture" \
        "cargo test -p codex-router-proxy assembled_loopback_router_runtime_routes_split_frame_previous_response_affinity -- --nocapture"
      ;;
    G-25)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Structural guard and metadata behavior passed: HTTP/SSE affinity extraction is bounded, and WebSocket routing metadata is observed after frame forwarding without gating pass-through." \
        "node verification/tooling/check-http-streaming-guard.ts G-25" \
        "cargo test -p codex-router-proxy async_http_affinity_scan_stops_at_explicit_bounds_without_gating_body -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_metadata_scan_extracts_oversized_late_affinity_and_completion_fields -- --nocapture"
      ;;
    G-26)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Structural guard and Hyper transport regression passed: /v1/models reaches upstream and no synthetic models payload remains in release code." \
        "CODEX_ROUTER_PROOF_VERIFY_ONLY=1 node verification/tooling/check-release-runtime-guardrails.ts G-26" \
        "cargo test -p codex-router-proxy hyper_http_upstream_transport_passes_backend_api_models_upstream -- --nocapture"
      ;;
    G-27)
      mark_row_pass "$row_id" "$layer" "$owner" \
        "Structural guard and first-frame regression passed: WebSocket pre-upstream parsing does not require prompt-bearing input/stream payload shape." \
        "CODEX_ROUTER_PROOF_VERIFY_ONLY=1 node verification/tooling/check-release-runtime-guardrails.ts G-27" \
        "cargo test -p codex-router-proxy websocket_first_future_json_payload_selects_and_forwards_unchanged -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_first_frame_accepts_future_request_shape_without_prompt_policy -- --nocapture" \
        "cargo test -p codex-router-proxy websocket_first_frame_rejects_hostile_preselection_cases -- --nocapture"
      ;;
    G-21)
      structural_hashes_before=$(mktemp "${TMPDIR:-/tmp}/codex-router-structural-before.XXXXXX")
      structural_hashes_after=$(mktemp "${TMPDIR:-/tmp}/codex-router-structural-after.XXXXXX")
      find "$EVIDENCE_ROOT/structural" -maxdepth 1 -type f -name 'G-*.json' \
        ! -name 'G-21.json' -print0 2>/dev/null \
        | sort -z \
        | xargs -0 shasum -a 256 > "$structural_hashes_before" || true
      for guardrail_row in G-01 G-02 G-03 G-04 G-05 G-06 G-07 G-08 G-09 G-10 G-11 G-12 G-13 G-14 G-15 G-16 G-17 G-18 G-19 G-20 G-22 G-23 G-24 G-25 G-26 G-27 G-28 G-29; do
        CODEX_ROUTER_PROOF_VERIFY_ONLY=1 "$0" "$guardrail_row" >/dev/null
      done
      find "$EVIDENCE_ROOT/structural" -maxdepth 1 -type f -name 'G-*.json' \
        ! -name 'G-21.json' -print0 2>/dev/null \
        | sort -z \
        | xargs -0 shasum -a 256 > "$structural_hashes_after" || true
      if ! cmp -s "$structural_hashes_before" "$structural_hashes_after"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; verify-only guardrail rows mutated persisted evidence; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      if ! ensure_guarded_source_paths_clean "$row_id"; then
        receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "fail" 1)
        printf 'proof row %s failed; guarded source paths are dirty; receipt: %s\n' "$row_id" "$receipt" >&2
        exit 1
      fi
      receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pass" 0)
      node verification/tooling/proof-matrix-receipt.ts patch "$receipt" "G-21"
      printf 'proof row %s passed; receipt: %s\n' "$row_id" "$receipt"
      exit 0
      ;;
  esac

  local receipt
  receipt=$(write_proof_receipt "$row_id" "$layer" "$owner" "pending_unimplemented" 3)
  printf 'proof row %s is pending; receipt: %s\n' "$row_id" "$receipt" >&2
  exit 3
}

main "$@"
