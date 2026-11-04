import assert from "node:assert/strict";
import { test } from "node:test";
import {
  journeyCheckSchema,
  type JourneyReadback,
  type NativeThread,
} from "./acp-journey-models.ts";
import { checkClaudeRoute, checkIdleJourney } from "./acp-journey-oracle.ts";
import {
  managedInventoryObservationSchema,
  parseManagedInventory,
  requireHistoryBefore,
  requireOneNewReplyTurn,
  type ManagedInventoryTarget,
} from "./claude-route-isolation.ts";
import type { ClaudeRouteWindow } from "./claude-route-window.ts";

// Literal shapes from the existing sources, never computed by the code under test: the
// collaboration CLI's one machine page (session_inventory_commands.rs:228-239;
// endpoint_commands.rs:12-77), hosted provider rows (provider_session_list.rs:57-68) and native
// thread history (v2 thread items).
const SERVICE_ID = "c4007a11-4c33-43a9-b44d-2a5ff96dd211";
const SESSION = "e4aa7dc9-8c96-4a78-8bcc-c87356410348";
const CWD = "/work/provider";
const TARGET: ManagedInventoryTarget = {
  collaborationCli: { path: "/unused/agent-collaboration", sha256: "0".repeat(64) },
  serviceDirectory: "/unused/service",
  serviceId: SERVICE_ID,
  serviceEpoch: "3d587954-ab51-40c6-b787-e104afa37558",
  sessionId: SESSION,
};

function row(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    origin: "hostedProvider",
    target: { endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" }, sessionId: SESSION },
    workingDirectory: CWD,
    updatedAt: 1793418868,
    state: "idle",
    requestedPolicy: { approvalPolicy: "policy-sentinel" },
    approver: { kind: "approver-sentinel" },
    createdBy: { kind: "creator-sentinel" },
    ...overrides,
  };
}
function page(records: readonly unknown[], pageOverrides: Record<string, unknown> = {}): string {
  return `${JSON.stringify({
    kind: "result",
    cliVersion: "0.1.118",
    result: {
      page: {
        endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" },
        observedAt: "2026-10-31T07:50:00.000Z",
        records,
        nextCursor: null,
        ...pageOverrides,
      },
    },
  })}\n`;
}

test("exactly the declared hosted Session, idle, keeps only identity, state, count and time", (): void => {
  const observed = parseManagedInventory(page([row()]), TARGET);
  assert.deepEqual(observed, {
    service_id: SERVICE_ID,
    endpoint_id: "claude-local",
    session_id: SESSION,
    state: "idle",
    count: 1,
    observed_at: "2026-10-31T07:50:00.000Z",
  });
  for (const leaked of ["policy-sentinel", "approver-sentinel", "creator-sentinel", CWD])
    assert.equal(JSON.stringify(observed).includes(leaked), false, leaked);
});

const OTHER = "019a0000-0000-7000-8000-0000000000aa";
const INVENTORY_FAILURES: ReadonlyArray<[string, string, RegExp]> = [
  [
    "another idle managed Session",
    page([
      row(),
      row({
        target: {
          endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" },
          sessionId: OTHER,
        },
      }),
    ]),
    /holds 2 Sessions/,
  ],
  [
    "another closed managed Session",
    page([
      row(),
      row({
        state: "closed",
        target: {
          endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" },
          sessionId: OTHER,
        },
      }),
    ]),
    /holds 2 Sessions/,
  ],
  [
    "another unloaded managed Session",
    page([
      row({
        state: "unloaded",
        target: {
          endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" },
          sessionId: OTHER,
        },
      }),
      row(),
    ]),
    /holds 2 Sessions/,
  ],
  ["no managed Session", page([]), /holds 0 Sessions/],
  [
    "a page for another service",
    page([row()], {
      endpoint: { serviceId: "019a0000-0000-7000-8000-0000000000bb", endpointId: "claude-local" },
    }),
    /another service or endpoint/,
  ],
  [
    "a page for another endpoint",
    page([row()], { endpoint: { serviceId: SERVICE_ID, endpointId: "cursor-local" } }),
    /another service or endpoint/,
  ],
  [
    "the only row for another Session",
    page([
      row({
        target: {
          endpoint: { serviceId: SERVICE_ID, endpointId: "claude-local" },
          sessionId: OTHER,
        },
      }),
    ]),
    /not the declared target/,
  ],
  [
    "the only row on another service",
    page([
      row({
        target: { endpoint: { serviceId: OTHER, endpointId: "claude-local" }, sessionId: SESSION },
      }),
    ]),
    /not the declared target/,
  ],
  [
    "a native interactive row",
    page([row({ origin: "claudeCodeInteractive" })]),
    /not one hosted provider record/,
  ],
  ["a running target", page([row({ state: "running" })]), /not idle/],
  ["an unloaded target", page([row({ state: "unloaded" })]), /not idle/],
  ["a closed target", page([row({ state: "closed" })]), /not idle/],
  [
    "another page follows",
    page([row()], { nextCursor: "eyJuZXh0IjoxfQ" }),
    /malformed or incomplete/,
  ],
  [
    "skipped records",
    page([row()], { skippedRecords: 1 }),
    /skipped records or read native sources/,
  ],
  [
    "native coverage",
    page([row()], { nativeCoverage: { peer: "complete", background: "complete" } }),
    /skipped records or read native sources/,
  ],
  [
    "an error envelope",
    `${JSON.stringify({ kind: "error", error: { code: -32050, data: { kind: "unavailable" } } })}\n`,
    /malformed or incomplete/,
  ],
  ["text before the document", `progress\n${page([row()])}`, /malformed or incomplete/],
  ["a second result after it", `${page([row()])}${page([row()])}`, /malformed or incomplete/],
  ["no output", "", /malformed or incomplete/],
];
for (const [name, stdout, expected] of INVENTORY_FAILURES)
  test(`the managed inventory refuses attribution: ${name}`, (): void => {
    assert.throws(() => parseManagedInventory(stdout, TARGET), expected);
  });

test("an unknown target state is refused without echoing it", (): void => {
  const secret = "sk-ant-api03-SECRET-STATE-TOKEN";
  assert.throws(
    () => parseManagedInventory(page([row({ state: secret })]), TARGET),
    (error: unknown) =>
      error instanceof Error && /not idle/.test(error.message) && !error.message.includes(secret),
  );
});

// ObservationTimestamp accepts chrono's strict RFC3339 ending in Z (endpoint_description.rs:52-67).
test("the observation time keeps only the source's RFC3339 UTC shape", (): void => {
  for (const time of [
    "2026-10-31T07:50:00Z",
    "2026-10-31t07:50:00.123456789Z",
    "2026-10-31 07:50:00.5Z",
    "2028-02-29T23:59:60Z",
  ])
    assert.equal(
      parseManagedInventory(page([row()], { observedAt: time }), TARGET).observed_at,
      time,
    );
  for (const time of [
    "2026-10-31T07:50:00+00:00",
    "2026-10-31T07:50:00z",
    "2026-10-31T07:50Z",
    "2026-02-29T07:50:00Z",
    "2026-10-31T24:00:00Z",
    "2026-13-01T07:50:00Z",
    "2026-10-31T07:50:00.Z",
    " 2026-10-31T07:50:00Z",
    "1793418868",
  ])
    assert.throws(
      () => parseManagedInventory(page([row()], { observedAt: time }), TARGET),
      /malformed or incomplete/,
      time,
    );
});

test("a secret-shaped observation time is refused without echoing it", (): void => {
  const secret = "sk-ant-api03-SECRET-TIME-TOKEN";
  assert.throws(
    () => parseManagedInventory(page([row()], { observedAt: secret }), TARGET),
    (error: unknown) =>
      error instanceof Error &&
      /malformed or incomplete/.test(error.message) &&
      !error.message.includes(secret),
  );
  // The decoded baseline observation keeps the same rule.
  const decoded = managedInventoryObservationSchema.safeParse({
    service_id: SERVICE_ID,
    endpoint_id: "claude-local",
    session_id: SESSION,
    state: "idle",
    count: 1,
    observed_at: secret,
  });
  assert.equal(decoded.success, false);
  assert.equal(JSON.stringify(decoded.error?.issues).includes(secret), false);
});

type Turn = NativeThread["turns"][number];
const PROMPT = "Compute 1 + 2345678. Reply with only the integer.";
const COMPLETION = "12345679";
const PRIOR: Turn = {
  id: "turn-1",
  status: "completed",
  items: [
    { id: "u1", type: "userMessage", content: [{ type: "text", text: "hello" }] },
    { id: "a1", type: "agentMessage", text: "HI" },
  ],
};
const REPLY: Turn = {
  id: "turn-2",
  status: "completed",
  items: [
    { id: "u2", type: "userMessage", content: [{ type: "text", text: PROMPT }] },
    { id: "a2", type: "agentMessage", text: COMPLETION },
  ],
};
const EXTRA: Turn = {
  id: "turn-x",
  status: "completed",
  items: [
    { id: "ux", type: "userMessage", content: [{ type: "text", text: "injected" }] },
    { id: "ax", type: "agentMessage", text: "other generation" },
  ],
};
const thread = (turns: Turn[], status = "idle", id = SESSION, cwd = CWD): NativeThread => ({
  id,
  cwd,
  status: { type: status },
  turns,
});
const BEFORE = thread([PRIOR]);

test("the final view must keep the prior turns and add exactly the completed reply turn", (): void => {
  assert.doesNotThrow(() => requireOneNewReplyTurn(BEFORE, thread([PRIOR, REPLY]), "turn-2"));
  for (const [name, after, expected] of [
    ["an extra completed turn before the reply", thread([PRIOR, EXTRA, REPLY]), /exactly one turn/],
    ["an extra turn after the reply", thread([PRIOR, REPLY, EXTRA]), /exactly one turn/],
    ["no new turn", thread([PRIOR]), /exactly one turn/],
    ["a prior turn replaced", thread([EXTRA, REPLY]), /prior turns changed/],
    [
      "a prior turn's text changed",
      thread([
        {
          ...PRIOR,
          items: [
            PRIOR.items[0] as Turn["items"][number],
            { id: "a1", type: "agentMessage", text: "HI there" },
          ],
        },
        REPLY,
      ]),
      /prior turns changed/,
    ],
    [
      "a prior turn's user content changed",
      thread([
        {
          ...PRIOR,
          items: [
            { id: "u1", type: "userMessage", content: [{ type: "text", text: "hullo" }] },
            PRIOR.items[1] as Turn["items"][number],
          ],
        },
        REPLY,
      ]),
      /prior turns changed/,
    ],
    [
      "a prior turn's status changed",
      thread([{ ...PRIOR, status: "interrupted" }, REPLY]),
      /prior turns changed/,
    ],
    [
      "a prior turn lost an item",
      thread([{ ...PRIOR, items: PRIOR.items.slice(0, 1) }, REPLY]),
      /prior turns changed/,
    ],
    [
      "the new turn still running",
      thread([PRIOR, { ...REPLY, status: "inProgress" }]),
      /not the completed reply turn/,
    ],
    ["the new turn is not the reply turn", thread([PRIOR, EXTRA]), /not the completed reply turn/],
  ] as const)
    assert.throws(() => requireOneNewReplyTurn(BEFORE, after, "turn-2"), expected, name);
});

test("the retained before-state must be the declared Session idle at its own cwd", (): void => {
  assert.doesNotThrow(() => requireHistoryBefore(BEFORE, SESSION, CWD));
  for (const before of [
    thread([PRIOR], "active"),
    thread([PRIOR], "notLoaded"),
    thread([PRIOR], "idle", OTHER),
    thread([PRIOR], "idle", SESSION, "/work/elsewhere"),
  ])
    assert.throws(() => requireHistoryBefore(before, SESSION, CWD), /retained before-state/);
});

const WINDOW: ClaudeRouteWindow = {
  baseline_state: "present",
  segment_bytes: 245,
  segment_sha256: "a".repeat(64),
  rows: 1,
  categories: [
    {
      route_kind: "claude_messages",
      transport_kind: "http",
      local_auth_result: "valid",
      outcome: "allowed",
      decision_reason: "forwarded",
      response_commit_state: "committed",
      error_class: null,
      account_hash_present: true,
      count: 1,
    },
  ],
};
const INVENTORY = {
  service_id: SERVICE_ID,
  endpoint_id: "claude-local",
  session_id: SESSION,
  state: "idle",
  count: 1,
  observed_at: "2026-10-31T07:50:00.000Z",
} as const;
const reply = journeyCheckSchema.parse({
  kind: "ordinary-reply",
  prompt: PROMPT,
  completion: COMPLETION,
});
const route = journeyCheckSchema.parse({
  kind: "claude-route",
  baseline: "/run/claude-route-baseline.json",
  before_thread_read: "/evidence/app-server-2.json",
});
function readback(
  overrides: Partial<JourneyReadback> = {},
  routeOverrides: Record<string, unknown> = {},
): JourneyReadback {
  const after = thread([PRIOR, REPLY]);
  return {
    unboundSkills: { data: [] },
    boundSkills: { data: [] },
    threadRead: after,
    threadResume: after,
    resumedModel: "haiku",
    models: null,
    information: null,
    claudeRoute: {
      serviceId: SERVICE_ID,
      window: WINDOW,
      inventoryBefore: INVENTORY,
      inventoryAfter: INVENTORY,
      historyBefore: BEFORE,
      ...routeOverrides,
    },
    providerAgentTexts: [COMPLETION],
    providerToolItems: [],
    ...overrides,
  };
}
const SESSION_TARGET = { endpointId: "claude-local", sessionId: SESSION, cwd: CWD };

test("a forwarded row, a genuine reply and an isolated single-turn window pass together", (): void => {
  const results = checkIdleJourney(readback(), SESSION_TARGET, [reply, route], () => BEFORE);
  assert.deepEqual(
    results.map((result) => result.kind),
    ["ordinary-reply", "claude-route"],
  );
  assert.equal(results[1]?.observed["reply_turn_id"], "turn-2");
  assert.equal(results[1]?.observed["prior_turns"], 1);
});

test("a forwarded row and a genuine reply never pass without the isolated single-turn window", (): void => {
  const foreign = { ...INVENTORY, service_id: "019a0000-0000-7000-8000-0000000000bb" };
  for (const [name, view, routeOverrides, expected] of [
    ["a foreign-service inventory before Enter", {}, { inventoryBefore: foreign }, /not isolated/],
    [
      "a foreign-service inventory after the reply",
      {},
      { inventoryAfter: foreign },
      /not isolated/,
    ],
    [
      "an inventory for another Session",
      {},
      { inventoryAfter: { ...INVENTORY, session_id: OTHER } },
      /not isolated/,
    ],
    [
      "an extra same-Session turn in read",
      { threadRead: thread([PRIOR, EXTRA, REPLY]) },
      {},
      /exactly one turn/,
    ],
    [
      "an extra same-Session turn only in resume",
      { threadResume: thread([PRIOR, EXTRA, REPLY]) },
      {},
      /exactly one turn/,
    ],
    [
      "a changed prior prefix",
      {
        threadRead: thread([{ ...PRIOR, status: "interrupted" }, REPLY]),
        threadResume: thread([{ ...PRIOR, status: "interrupted" }, REPLY]),
      },
      {},
      /prior turns changed/,
    ],
    ["a wrong before-state", {}, { historyBefore: thread([]) }, /exactly one turn/],
  ] as const)
    assert.throws(
      () =>
        checkIdleJourney(
          readback(view, routeOverrides),
          SESSION_TARGET,
          [reply, route],
          () => BEFORE,
        ),
      expected,
      name,
    );
  // The reply turn the route rule requires is the one the plan's own reply check established.
  assert.throws(
    () => checkClaudeRoute(readback(), { sessionId: SESSION }, "turn-x"),
    /not the completed reply turn/,
  );
  assert.throws(
    () => checkIdleJourney(readback(), SESSION_TARGET, [route], () => BEFORE),
    /its plan's own genuine reply check/,
  );
});
