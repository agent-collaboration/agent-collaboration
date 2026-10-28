import {
  atLeast,
  display,
  equalNumber,
  integer,
  list,
  section,
  truthy,
  type ProofRecord,
} from "./proof-artifact-values.ts";
import { continuityErrors } from "./proof-continuity-checks.ts";

type Check = readonly [boolean, string];
export function failed(checks: readonly Check[]): string[] {
  return checks.filter(([passed]) => !passed).map(([, message]) => message);
}
function countErrors(upstream: ProofRecord, registry: ProofRecord): string[] {
  const errors: string[] = [];
  let counts = registry.final_session_forwarded_upstream_message_counts ?? [];
  if (!Array.isArray(counts)) {
    errors.push("final_session_forwarded_upstream_message_counts is not a list");
    counts = [];
  }
  if (list(counts).filter((count) => integer(count) && atLeast(count, 3)).length < 3)
    errors.push(
      `final-session forwarded counts do not prove three unique sessions with >=3 frames: ${display(counts)}`,
    );
  let events = upstream.in_overlap_session_event_counts ?? [];
  if (!Array.isArray(events)) {
    errors.push("in_overlap_session_event_counts is not a list");
    events = [];
  }
  if (list(events).filter((count) => integer(count) && atLeast(count, 3)).length < 3)
    errors.push(
      `upstream in_overlap_session_event_counts do not prove three unique non-prewarm sessions with >=3 in-overlap events: ${display(events)}`,
    );
  if (!atLeast(registry.forwarded_upstream_messages, 9))
    errors.push("forwarded_upstream_messages is below 9");
  return errors;
}
function firstRowErrors(artifact: ProofRecord, upstream: ProofRecord): string[] {
  const router = section(artifact, "router_process");
  const errors = failed([
    [
      artifact.shared_router_pid === router.pid,
      "shared_router_pid does not match router_process.pid",
    ],
    [
      router.spawned_real_serve_child === true,
      "router_process.spawned_real_serve_child is not true",
    ],
    [atLeast(upstream.active_high_water, 3), "upstream active_high_water is below 3"],
    [
      upstream.multi_step_interleave_completed === true,
      "multi_step_interleave_completed is not true",
    ],
  ]);
  for (const value of list(section(artifact, "clients").statuses)) {
    const markers = section({ value }, "value").stderr_transport_error_markers;
    if (!Array.isArray(markers))
      errors.push("client status is missing stderr_transport_error_markers");
    else if (markers.length !== 0)
      errors.push(`client stderr contains WebSocket transport error markers: ${display(markers)}`);
  }
  return errors;
}
function closeErrors(upstream: ProofRecord, cleanup: ProofRecord): string[] {
  const errors = failed([
    [atLeast(upstream.normal_close_sessions, 3), "upstream normal_close_sessions is below 3"],
    [equalNumber(upstream.abnormal_close_sessions, 0), "upstream abnormal_close_sessions is not 0"],
  ]);
  const outcomes = upstream.session_close_outcomes ?? [];
  if (!Array.isArray(outcomes) || outcomes.length < 3)
    errors.push("upstream session_close_outcomes is missing or incomplete");
  else if (outcomes.some((outcome) => outcome !== "normal"))
    errors.push(
      `upstream session_close_outcomes contains non-normal outcomes: ${display(outcomes)}`,
    );
  return [
    ...errors,
    ...failed([
      [equalNumber(cleanup.established_count, 0), "socket cleanup established_count is not 0"],
      [equalNumber(cleanup.close_wait_count, 0), "socket cleanup close_wait_count is not 0"],
      [Array.isArray(cleanup.raw_state_counts), "socket cleanup raw_state_counts is missing"],
    ]),
  ];
}
function privacyErrors(artifact: ProofRecord, rendered: string): string[] {
  const forbidden = [
    "installed-smoke-upstream-token",
    "installed-smoke-matches-token",
    "local-token",
    "CODEX_ROUTER_TOKEN=",
    "Bearer ",
    "prompt-canary",
    "raw-previous-response-id-canary",
    "response.create",
    "codex-router smoke ok",
    "acct_",
    "askluna",
    "matches",
    "ssdev",
  ];
  const errors = forbidden
    .filter((fragment) => rendered.includes(fragment))
    .map((fragment) => `artifact leaked forbidden fragment: ${fragment}`);
  const allowed = new Set([
    "clients",
    "codex_version",
    "git_head",
    "mode",
    "router_process",
    "router_websocket_registry",
    "runtime_correlations",
    "session_continuity",
    "shared_router_pid",
    "socket_cleanup",
    "upstream",
  ]);
  const extra = Object.keys(artifact)
    .filter((name) => !allowed.has(name))
    .sort();
  if (extra.length !== 0)
    errors.push(`artifact contains non-allowlisted top-level keys: ${display(extra)}`);
  return errors;
}
export function soakRowErrors(row: string, artifact: ProofRecord, rendered: string): string[] {
  const upstream = section(artifact, "upstream");
  const registry = section(artifact, "router_websocket_registry");
  switch (row) {
    case "E-01":
      return firstRowErrors(artifact, upstream);
    case "E-02":
      return failed([
        [atLeast(upstream.hold_duration_ms, 300000), "hold_duration_ms is below five minutes"],
        [atLeast(upstream.active_high_water, 3), "upstream active_high_water is below 3"],
        [
          atLeast(registry.high_water_sessions, 3),
          "router registry high_water_sessions is below 3",
        ],
      ]);
    case "E-03":
      return failed([
        [upstream.overlap_proven === true, "overlap_proven is not true"],
        [
          atLeast(upstream.real_overlap_duration_ms, 300000),
          "real_overlap_duration_ms is below five minutes",
        ],
        [
          truthy(upstream.overlap_started_unix_ms) &&
            truthy(upstream.real_overlap_completed_unix_ms),
          "real overlap timestamps are missing",
        ],
      ]);
    case "E-04":
      return countErrors(upstream, registry);
    case "E-05":
      return failed([
        [
          upstream.multi_step_interleave_completed === true,
          "multi_step_interleave_completed is not true",
        ],
        [
          atLeast(upstream.multi_step_followup_frame_count, 1),
          "multi_step_followup_frame_count is below 1",
        ],
        [
          atLeast(upstream.multi_step_followup_active_session_count, 3),
          "multi_step_followup_active_session_count is below 3",
        ],
        [
          upstream.multi_step_completed_before_overlap_end === true,
          "multi_step_completed_before_overlap_end is not true for real 3-way overlap",
        ],
      ]);
    case "E-06":
      return failed([
        [
          equalNumber(registry.handled_connections, 3),
          "router final report handled_connections is not 3",
        ],
        [equalNumber(registry.active_sessions, 0), "router registry active_sessions is not 0"],
        [
          atLeast(registry.high_water_sessions, 3),
          "router registry high_water_sessions is below 3",
        ],
        [atLeast(registry.closed_sessions, 3), "router registry closed_sessions is below 3"],
        [
          atLeast(registry.completed_response_sessions, 3),
          "router registry completed_response_sessions is below 3",
        ],
      ]);
    case "E-07":
      return continuityErrors(artifact);
    case "E-08":
      return closeErrors(upstream, section(artifact, "socket_cleanup"));
    case "E-09":
      return privacyErrors(artifact, rendered);
    default:
      return [];
  }
}
