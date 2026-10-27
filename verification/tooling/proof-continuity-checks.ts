import { isDeepStrictEqual } from "node:util";
import {
  atLeast,
  display,
  equalNumber,
  integer,
  section,
  truthy,
  type ProofRecord,
} from "./proof-artifact-values.ts";

function arrayField(
  record: ProofRecord,
  field: string,
  failure: string,
  errors: string[],
): unknown[] {
  const value = record[field];
  if (!Array.isArray(value) || value.length < 3) {
    errors.push(failure);
    return [];
  }
  return value;
}
function isRecord(value: unknown): value is ProofRecord {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function sorted(values: Iterable<unknown>): unknown[] {
  return [...values].sort((a, b) =>
    typeof a === "bigint" && typeof b === "bigint"
      ? a < b
        ? -1
        : a > b
          ? 1
          : 0
      : String(a).localeCompare(String(b)),
  );
}
function sameSet(left: Set<unknown>, right: Set<unknown>): boolean {
  const key = (value: unknown): unknown =>
    typeof value === "boolean" || (typeof value === "number" && Number.isInteger(value))
      ? BigInt(Number(value))
      : value;
  return isDeepStrictEqual(new Set([...left].map(key)), new Set([...right].map(key)));
}
function joinErrors(input: {
  readonly continuity: ProofRecord;
  readonly registry: ProofRecord;
  readonly upstreamIds: readonly unknown[];
}): string[] {
  const { continuity, registry, upstreamIds } = input;
  const errors: string[] = [];
  let keys = continuity.per_client_join_keys;
  if (!Array.isArray(keys) || keys.length !== 3) {
    errors.push("session_continuity per_client_join_keys must contain exactly three entries");
    keys = [];
  }
  const indexes = new Set<unknown>();
  const routerIds = new Set<unknown>();
  const upstreamJoins = new Set<unknown>();
  for (const [index, key] of (keys as unknown[]).entries()) {
    if (!isRecord(key)) {
      errors.push(`per_client_join_keys[${index}] is not an object`);
      continue;
    }
    indexes.add(key.client_index ?? null);
    for (const field of ["client_pid", "router_session_id", "upstream_session_id"])
      if (key[field] === undefined || key[field] === null || key[field] === "")
        errors.push(`per_client_join_keys[${index}] missing ${field}`);
    if (integer(key.router_session_id)) routerIds.add(key.router_session_id);
    if (integer(key.upstream_session_id)) upstreamJoins.add(key.upstream_session_id);
  }
  if (!sameSet(indexes, new Set([0n, 1n, 2n])))
    errors.push(`per_client_join_keys client indexes are wrong: ${display(sorted(indexes))}`);
  const registered = Array.isArray(registry.registered_session_ids)
    ? registry.registered_session_ids
    : [];
  const closed = Array.isArray(registry.closed_session_ids) ? registry.closed_session_ids : [];
  const registeredSet = new Set(registered);
  const closedSet = new Set(closed);
  const upstreamSet = new Set(upstreamIds);
  if (registeredSet.size < 3)
    errors.push(
      `router registered_session_ids do not contain three unique ids: ${display(registered)}`,
    );
  if (closedSet.size < 3)
    errors.push(`router closed_session_ids do not contain three unique ids: ${display(closed)}`);
  if (upstreamSet.size < 3)
    errors.push(`upstream_session_ids do not contain three unique ids: ${display(upstreamIds)}`);
  if (!sameSet(routerIds, registeredSet))
    errors.push(
      `joined router session ids ${display(sorted(routerIds))} do not match registered ids ${display(sorted(registeredSet))}`,
    );
  if (!sameSet(upstreamJoins, upstreamSet))
    errors.push(
      `joined upstream session ids ${display(sorted(upstreamJoins))} do not match upstream ids ${display(sorted(upstreamSet))}`,
    );
  for (const [field, count, message] of [
    [
      "router_registered_unique_session_count",
      registeredSet.size,
      "session_continuity router_registered_unique_session_count does not match registry",
    ],
    [
      "router_closed_unique_session_count",
      closedSet.size,
      "session_continuity router_closed_unique_session_count does not match registry",
    ],
    [
      "upstream_unique_session_count",
      upstreamSet.size,
      "session_continuity upstream_unique_session_count does not match upstream",
    ],
  ] as const)
    if (!equalNumber(continuity[field], count)) errors.push(message);
  return errors;
}
function correlationErrors(artifact: ProofRecord, router: ProofRecord): string[] {
  const errors: string[] = [];
  let correlations = artifact.runtime_correlations;
  if (!Array.isArray(correlations) || correlations.length !== 3) {
    errors.push("runtime_correlations must contain exactly three client process entries");
    correlations = [];
  }
  for (const [index, value] of (correlations as unknown[]).entries()) {
    if (!isRecord(value)) {
      errors.push(`runtime correlation ${index} is not an object`);
      continue;
    }
    for (const field of [
      "client_pid",
      "router_pid",
      "router_session_id",
      "upstream_session_id",
      "transport",
      "stderr_transport_error_markers",
      "stdout_contains_smoke_text",
    ])
      if (value[field] === undefined || value[field] === null || value[field] === "")
        errors.push(`runtime correlation ${index} missing ${field}`);
    if (value.transport !== "websocket")
      errors.push(`runtime correlation ${index} transport is not websocket`);
    if (
      !Array.isArray(value.stderr_transport_error_markers) ||
      value.stderr_transport_error_markers.length !== 0
    )
      errors.push(`runtime correlation ${index} has transport error markers`);
    if (value.stdout_contains_smoke_text !== true)
      errors.push(`runtime correlation ${index} stdout did not contain smoke text`);
    if (value.router_pid !== router.pid)
      errors.push(`runtime correlation ${index} router_pid does not match router process`);
  }
  return errors;
}
export function continuityErrors(artifact: ProofRecord): string[] {
  const errors: string[] = [];
  const router = section(artifact, "router_process");
  const upstream = section(artifact, "upstream");
  const registry = section(artifact, "router_websocket_registry");
  for (const field of ["binary_path", "argv", "listener", "readiness_line"])
    if (!truthy(router[field])) errors.push(`router_process.${field} is missing`);
  if (artifact.shared_router_pid !== router.pid)
    errors.push("shared router PID correlation is missing");
  if (!equalNumber(upstream.expected_sessions, 3) || !equalNumber(upstream.completed_sessions, 3))
    errors.push("upstream session count correlation is incomplete");
  if (!atLeast(registry.registered_sessions, 3) || !atLeast(registry.closed_sessions, 3))
    errors.push("router registry session correlation is incomplete");
  for (const field of ["registered_session_ids", "completed_session_ids", "closed_session_ids"])
    arrayField(registry, field, `router registry ${field} missing observed session ids`, errors);
  const upstreamIds = arrayField(
    upstream,
    "upstream_session_ids",
    "upstream_session_ids missing observed upstream session ids",
    errors,
  );
  arrayField(
    registry,
    "final_session_forwarded_upstream_message_counts",
    "router registry final per-session counters are missing",
    errors,
  );
  if (!equalNumber(upstream.http_probe_count, 0))
    errors.push("unexpected HTTP probe count in WebSocket continuity artifact");
  let continuity = artifact.session_continuity;
  if (!isRecord(continuity)) {
    errors.push("session_continuity object is missing");
    continuity = {};
  }
  const details = section({ continuity }, "continuity");
  if (details.per_client_session_join_key_observed !== true)
    errors.push("session_continuity did not prove per-client session join keys");
  if (details.correlation_level !== "per_client_socket_and_marker_join")
    errors.push("session_continuity correlation_level is not per_client_socket_and_marker_join");
  errors.push(
    ...joinErrors({ continuity: details, registry, upstreamIds }),
    ...correlationErrors(artifact, router),
  );
  return errors;
}
