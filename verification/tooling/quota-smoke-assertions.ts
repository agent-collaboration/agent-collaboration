import assert from "node:assert/strict";
import { z } from "zod";
import { evidenceJson } from "../evidence-json.ts";
import {
  atLeast,
  equalNumber,
  list,
  readProof,
  record,
  section,
  truthy,
  type ProofRecord,
} from "./proof-artifact-values.ts";

function accounts(payload: ProofRecord): {
  readonly rows: ProofRecord[];
  readonly labels: Readonly<Record<string, ProofRecord>>;
} {
  const rows = z.array(z.record(z.string(), z.unknown())).parse(payload.accounts);
  return {
    rows,
    labels: Object.fromEntries(
      rows.map((account) => [z.string().parse(account.safe_account_label), account]),
    ),
  };
}
function named(labels: Readonly<Record<string, ProofRecord>>, name: string): ProofRecord {
  return record(labels[name]);
}
function oneOf(value: unknown, allowed: readonly string[]): void {
  assert.ok(allowed.includes(z.string().parse(value)));
}
function absent(value: ProofRecord, names: readonly string[]): void {
  for (const name of names) assert.ok(!Object.hasOwn(value, name), name);
}
function present(value: ProofRecord, names: readonly string[]): void {
  for (const name of names) assert.ok(Object.hasOwn(value, name), name);
}
function statusAssertions(payload: ProofRecord): void {
  assert.equal(payload.route_band, "responses");
  oneOf(payload.selected_pool, ["usable", "reserve", "none"]);
  const { rows, labels } = accounts(payload);
  assert.equal(rows.length, 3);
  assert.equal(named(labels, "askluna").availability, "blocked");
  assert.equal(named(labels, "askluna").next_use, "blocked");
  for (const name of ["matches", "ssdev"])
    oneOf(named(labels, name).availability, ["usable", "reserve"]);
  absent(payload, ["weighted_candidates"]);
  const ssdev = named(labels, "ssdev");
  absent(ssdev, ["routing_weight"]);
  assert.ok(equalNumber(ssdev.weekly_quota_guard, 3));
  present(ssdev, [
    "weekly_survival_margin_basis_points",
    "weekly_projected_exhaustion_unix_seconds",
    "current_active_sessions",
    "hard_block_reason",
  ]);
  oneOf(ssdev.short_guard_result, ["pass", "held", "unknown"]);
  oneOf(ssdev.active_session_source, ["sqlx_mirror", "unavailable"]);
  oneOf(ssdev.weekly_burn_rate_confidence, ["unknown", "insufficient", "low", "normal", "stale"]);
  absent(named(labels, "matches"), ["routing_weight"]);
  assert.ok(equalNumber(named(labels, "matches").weekly_quota_guard, 32));
  for (const account of rows) {
    assert.equal(z.array(z.unknown()).parse(account.windows).length, 2);
    absent(account, ["routing_weight", "short_quota_risk", "weekly_quota_risk"]);
    present(account, ["short_quota_guard", "weekly_quota_guard"]);
    oneOf(account.routing_reason, [
      "preferred_weekly_healthier",
      "preferred_weekly_reset_soon",
      "preferred_short_reset_soon",
      "preferred_safest_quota",
      "available_same_pool",
      "held_reserve",
      "held_unknown",
      "held_short_window_guard",
      "unknown_fallback_preferred",
      "unknown_fallback_available",
      "excluded_disabled",
      "excluded_missing_credential",
      "blocked_window_exhausted",
      "blocked_window_ineligible",
    ]);
  }
}
function selectionAssertions(payload: ProofRecord, planOne: boolean): void {
  assert.equal(payload.route_band, "responses");
  const { rows, labels } = accounts(payload);
  const preferred = rows.filter((account) => {
    assert.ok(Object.hasOwn(account, "preferred_next"));
    return truthy(account.preferred_next);
  });
  assert.equal(preferred.length, 1);
  const best = z.record(z.string(), z.unknown()).parse(preferred[0]);
  assert.equal(best.safe_account_label, planOne ? "best" : "askluna");
  if (!planOne) {
    assert.equal(payload.selected_pool, "usable");
    absent(payload, ["weighted_candidates"]);
  }
  const names = planOne ? ["weak", "mid", "best"] : ["askluna", "matches", "ssdev"];
  for (const [index, name] of names.entries()) {
    const account = named(labels, name);
    assert.ok(equalNumber(account.active_clients, index === 2 ? 0 : 1));
    assert.equal(account.active_clients_source, "sqlx_mirror");
    if (!planOne) {
      assert.equal(account.preferred_next, index === 0);
      assert.ok(equalNumber(account.weekly_quota_guard, index === 0 ? 20 : index === 1 ? 10 : 3));
      absent(account, ["routing_weight"]);
    }
  }
  const winner = named(labels, planOne ? "best" : "askluna");
  assert.equal(winner.next_use, "preferred by quota");
  assert.equal(
    winner.routing_reason,
    planOne ? "preferred_weekly_healthier" : "preferred_safest_quota",
  );
  if (!planOne)
    for (const account of rows) {
      absent(account, ["short_quota_risk", "weekly_quota_risk", "routing_weight"]);
      present(account, ["short_quota_guard", "weekly_quota_guard"]);
    }
}
function privateTextAbsent(payload: ProofRecord, forbidden: readonly string[]): void {
  const text = evidenceJson(payload, true);
  for (const fragment of forbidden) assert.ok(!text.includes(fragment), fragment);
}
function reconnectAssertions(payload: ProofRecord): void {
  assert.ok(truthy(payload.git_head), "artifact must record git head");
  const codex = section(payload, "codex");
  assert.equal(codex.status, "exit status: 0");
  assert.equal(codex.stdout_contains_smoke_text, true);
  assert.equal(codex.stdout_contains_usage_limit_reached, false);
  assert.equal(codex.stderr_contains_usage_limit_reached, false);
  assert.equal(section(payload, "quota_reconnect").reconnected_to_different_account, true);
  assert.equal(section(payload, "quota_reconnect").quota_error_hidden_from_codex, true);
  assert.equal(payload.profile_uses_codex_router_token, false);
  assert.ok(equalNumber(section(payload, "upstream").websocket_handshake_count, 2));
  assert.ok(atLeast(section(payload, "upstream").non_prewarm_frame_count, 2));
  privateTextAbsent(payload, [
    "acct_quota_primary",
    "acct_quota_fallback",
    "quota-primary",
    "quota-fallback",
    "primary-token",
    "fallback-token",
    "X-Codex-Router-Token",
  ]);
}
function installedAssertions(payload: ProofRecord): void {
  const router = section(payload, "router_process");
  const clients = section(payload, "clients");
  const upstream = section(payload, "upstream");
  assert.equal(router.spawned_real_serve_child, true);
  assert.deepEqual(payload.shared_router_pid, router.pid);
  assert.ok(equalNumber(clients.count, 3));
  assert.equal(clients.all_success, true);
  assert.ok(atLeast(upstream.active_high_water, 3));
  assert.equal(upstream.overlap_proven, true);
  assert.ok(equalNumber(upstream.target_model_session_count, 3));
  for (const value of z.array(z.unknown()).parse(clients.statuses)) {
    const status = record(value);
    assert.equal(status.status, "exit status: 0");
    assert.equal(status.stdout_contains_smoke_text, true);
  }
}
export const requiredMetricLabels: Readonly<Record<string, readonly string[]>> = {
  codex_router_account_selections_total: [
    "account.slot",
    "agent.proof.marker",
    "route_band",
    "selection.reason",
    "service.name",
    "transport",
  ],
  codex_router_account_rejections_total: [
    "account.slot",
    "agent.proof.marker",
    "route_band",
    "selection.reason",
    "service.name",
    "transport",
  ],
  codex_router_active_clients: [
    "account.slot",
    "agent.proof.marker",
    "route_band",
    "service.name",
    "transport",
  ],
  codex_router_quota_refresh_total: [
    "agent.proof.marker",
    "refresh.error_class",
    "refresh.outcome",
    "route_band",
    "service.name",
  ],
  codex_router_websocket_events_total: [
    "agent.proof.marker",
    "event.kind",
    "route_band",
    "service.name",
  ],
  codex_router_quota_remaining_bucket: [
    "account.slot",
    "agent.proof.marker",
    "quota.remaining_bucket",
    "quota.window",
    "route_band",
    "service.name",
  ],
  codex_router_quota_pressure_bucket: [
    "account.slot",
    "agent.proof.marker",
    "quota.pressure_bucket",
    "quota.window",
    "route_band",
    "service.name",
  ],
};
function metricsAssertions(payload: ProofRecord, secret: string): void {
  privateTextAbsent(payload, [
    secret,
    "acct_retire",
    "acct_reserve",
    "acct_ssdev",
    "acct_unknown",
    "reservation-retire-1",
    "reservation-reserve-1",
    "access-token",
    "refresh-token",
    "authorization",
    "X-Codex-Router-Token",
  ]);
  const seen = new Set<string>();
  const keys = new Set<string>();
  for (const value of list(payload.data)) {
    const series = record(value);
    for (const name of Object.keys(series)) keys.add(name);
    const name = series.__name__;
    if (typeof name !== "string" || requiredMetricLabels[name] === undefined) continue;
    seen.add(name);
    for (const label of requiredMetricLabels[name])
      assert.ok(Object.hasOwn(series, label), `${name} missing ${label}`);
  }
  assert.deepEqual([...seen].sort(), Object.keys(requiredMetricLabels).sort());
  for (const name of [
    "account.id",
    "account.label",
    "reservation.id",
    "route.path",
    "prompt",
    "payload",
    "token",
    "provider.body",
  ])
    assert.ok(!keys.has(name), name);
}
export function quotaSmokeAssertions(
  mode: string,
  payload: ProofRecord,
  secret: string = "",
): boolean {
  switch (mode) {
    case "status":
      statusAssertions(payload);
      break;
    case "selection":
      selectionAssertions(payload, false);
      break;
    case "plan1":
      selectionAssertions(payload, true);
      break;
    case "plan2":
      reconnectAssertions(payload);
      break;
    case "plan3-installed":
      installedAssertions(payload);
      break;
    case "metrics":
      metricsAssertions(payload, secret);
      break;
    case "metric-present":
      return truthy(payload.data);
    default:
      throw new Error("unknown quota smoke assertion mode");
  }
  return true;
}
if (import.meta.main)
  process.exitCode = quotaSmokeAssertions(
    z.string().parse(process.argv[2]),
    readProof(z.string().parse(process.argv[3])),
    process.argv[4],
  )
    ? 0
    : 1;
