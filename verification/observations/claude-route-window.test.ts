import assert from "node:assert/strict";
import {
  appendFileSync,
  chmodSync,
  readFileSync,
  renameSync,
  symlinkSync,
  truncateSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import type { JourneyCheckResult } from "./acp-journey-models.ts";
import { readPlanClaudeRoute } from "./acp-journey-observation.ts";
import { checkClaudeRoute } from "./acp-journey-oracle.ts";
import {
  captureClaudeRouteBaseline,
  readClaudeRouteWindow,
  type ClaudeRouteBinding,
  type ClaudeRouteWindow,
} from "./claude-route-window.ts";
import {
  ACCOUNT_HASH,
  FORWARDED_CLAUDE,
  INVENTORY,
  MODELS_FORWARDED,
  OTHER_SESSION,
  PORT,
  PRIOR_TURN,
  REJECTED_CLAUDE,
  REPLY_TURN,
  RESTARTED_EPOCH,
  SERVICE_ID,
  SESSION,
  TARGET,
  observerPlan,
  routeFixture,
  sessionThread,
  type RouteFixture,
} from "./claude-route-fixture.ts";

/** The window inside an otherwise valid isolated route: both reads idle, one new reply turn. */
function routeResult(window: ClaudeRouteWindow): JourneyCheckResult {
  const before = sessionThread([PRIOR_TURN]);
  const after = sessionThread([PRIOR_TURN, REPLY_TURN]);
  return checkClaudeRoute(
    {
      claudeRoute: {
        serviceId: SERVICE_ID,
        window,
        inventoryBefore: INVENTORY,
        inventoryAfter: INVENTORY,
        historyBefore: before,
      },
      threadRead: after,
      threadResume: after,
    },
    { sessionId: SESSION },
    "turn-2",
  );
}

test("a forwarded Claude row appended after the baseline passes and keeps only categories", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baseline = captureClaudeRouteBaseline(fixture.binding, TARGET);
  assert.equal(baseline.file.state, "present");
  if (baseline.file.state !== "present") throw new Error("baseline missing");
  assert.equal(baseline.file.byte_length, Buffer.byteLength(MODELS_FORWARDED));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE + MODELS_FORWARDED);
  const window = readClaudeRouteWindow(baseline, fixture.binding);
  const result = routeResult(window);
  assert.deepEqual(
    {
      baseline: result.observed["baseline"],
      rows: result.observed["rows"],
      forwarded: result.observed["forwarded_claude"],
      rejected: result.observed["rejected_claude"],
      other: result.observed["other_route_rows"],
      bytes: result.observed["segment_bytes"],
    },
    {
      baseline: "present",
      rows: 2,
      forwarded: 1,
      rejected: 0,
      other: 1,
      bytes: Buffer.byteLength(FORWARDED_CLAUDE + MODELS_FORWARDED),
    },
  );
  // Safe redaction: no account hash value or request ID survives in any retained view.
  for (const retained of [
    JSON.stringify(window),
    JSON.stringify(result),
    JSON.stringify(baseline),
  ]) {
    assert.equal(retained.includes(ACCOUNT_HASH), false);
    assert.equal(retained.includes("acct_openai_77"), false);
    assert.equal(retained.includes("local_proxy_request"), false);
  }
});

test("an audit file the Router creates only after the baseline is read whole", (context): void => {
  const fixture = routeFixture(context, null);
  const baseline = captureClaudeRouteBaseline(fixture.binding, TARGET);
  assert.deepEqual(baseline.file, { state: "absent" });
  writeFileSync(fixture.auditFile, FORWARDED_CLAUDE, { mode: 0o600 });
  const result = routeResult(readClaudeRouteWindow(baseline, fixture.binding));
  assert.equal(result.observed["baseline"], "absent");
  assert.equal(result.observed["forwarded_claude"], 1);
});

const WINDOW_FAILURES: ReadonlyArray<[string, (fixture: RouteFixture) => void, RegExp]> = [
  [
    "a forwarded Claude row over WebSocket, which the Claude edge never appends",
    (f) =>
      appendFileSync(
        f.auditFile,
        FORWARDED_CLAUDE.replace('"transport_kind":"http"', '"transport_kind":"web_socket"'),
      ),
    /no forwarded Claude row/,
  ],
  [
    "a forwarded Claude row without an account hash",
    (f) =>
      appendFileSync(
        f.auditFile,
        FORWARDED_CLAUDE.replace(`,"account_hash":"${ACCOUNT_HASH}"`, ""),
      ),
    /no forwarded Claude row/,
  ],
  [
    "models-only segment",
    (f) => appendFileSync(f.auditFile, MODELS_FORWARDED),
    /no forwarded Claude row/,
  ],
  ["nothing appended", () => undefined, /no forwarded Claude row/],
  [
    "a rejected Claude row beside a forwarded one",
    (f) => appendFileSync(f.auditFile, FORWARDED_CLAUDE + REJECTED_CLAUDE),
    /1 rejected Claude row/,
  ],
  ["the file is missing", (f) => unlinkSync(f.auditFile), /missing after the reply/],
  ["a non-JSON row", (f) => appendFileSync(f.auditFile, "not json\n"), /malformed row/],
  [
    "an unknown field",
    (f) =>
      appendFileSync(f.auditFile, FORWARDED_CLAUDE.replace('"outcome"', '"note":"x","outcome"')),
    /malformed row/,
  ],
  [
    "an unknown route kind",
    (f) => appendFileSync(f.auditFile, FORWARDED_CLAUDE.replace("claude_messages", "claude_beta")),
    /malformed row/,
  ],
  [
    "free text in a reason",
    (f) => appendFileSync(f.auditFile, FORWARDED_CLAUDE.replace('"forwarded"', '"Bearer abc123"')),
    /malformed row/,
  ],
  [
    "a row cut mid-write",
    (f) => appendFileSync(f.auditFile, FORWARDED_CLAUDE.slice(0, 40)),
    /ends inside a row/,
  ],
  [
    "the file replaced by another",
    (f) => {
      writeFileSync(`${f.auditFile}.next`, MODELS_FORWARDED + FORWARDED_CLAUDE, { mode: 0o600 });
      renameSync(`${f.auditFile}.next`, f.auditFile);
    },
    /replaced after the baseline/,
  ],
  ["the file truncated", (f) => truncateSync(f.auditFile, 10), /truncated after the baseline/],
  [
    "the prefix rewritten",
    (f) => {
      writeFileSync(f.auditFile, MODELS_FORWARDED.replace("models", "compact"), { flag: "r+" });
      appendFileSync(f.auditFile, FORWARDED_CLAUDE);
    },
    /prefix changed after the baseline/,
  ],
  [
    "a segment over its bound",
    (f) => appendFileSync(f.auditFile, FORWARDED_CLAUDE.repeat(20)),
    /exceeds its declared bound/,
  ],
];

for (const [name, mutate, expected] of WINDOW_FAILURES)
  test(`the route window fails without retry: ${name}`, (context): void => {
    const fixture = routeFixture(context, MODELS_FORWARDED);
    const baseline = captureClaudeRouteBaseline(fixture.binding, TARGET);
    mutate(fixture);
    assert.throws(() => routeResult(readClaudeRouteWindow(baseline, fixture.binding)), expected);
  });

const BINDING_FAILURES: ReadonlyArray<
  [string, (fixture: RouteFixture) => ClaudeRouteBinding, RegExp]
> = [
  ["another Host process", (f) => ({ ...f.binding, hostPid: 5447 }), /does not name the declared/],
  [
    "another service directory",
    (f) => {
      f.marker({ serviceDirectory: join(f.root, "other") });
      return f.binding;
    },
    /does not name the declared/,
  ],
  [
    "another run directory in the marker",
    (f) => {
      f.marker({ runDirectory: join(f.root, "elsewhere") });
      return f.binding;
    },
    /does not name the declared/,
  ],
  [
    "a Host without a Claude provider",
    (f) => {
      f.marker({ claudeProvider: null });
      return f.binding;
    },
    /does not name the declared/,
  ],
  [
    "another Router listener",
    (f) => {
      f.marker({ port: PORT + 1 });
      return f.binding;
    },
    /published listener/,
  ],
  [
    "a shared run directory",
    (f) => {
      chmodSync(f.runDirectory, 0o755);
      return f.binding;
    },
    /owner-private canonical directory/,
  ],
  [
    "a linked run directory",
    (f) => {
      symlinkSync(f.runDirectory, join(f.root, "linked"));
      return { ...f.binding, hostRunDirectory: join(f.root, "linked") };
    },
    /owner-private canonical directory/,
  ],
  [
    "a linked audit file",
    (f) => {
      renameSync(f.auditFile, join(f.root, "real-audit.jsonl"));
      symlinkSync(join(f.root, "real-audit.jsonl"), f.auditFile);
      return f.binding;
    },
    /owner-private regular file/,
  ],
  [
    "a readable audit file",
    (f) => {
      chmodSync(f.auditFile, 0o644);
      return f.binding;
    },
    /owner-private regular file/,
  ],
  [
    "a prefix over its bound",
    (f) => ({ ...f.binding, maxPrefixBytes: 10 }),
    /prefix exceeds its declared bound/,
  ],
];

for (const [name, rebind, expected] of BINDING_FAILURES)
  test(`the route baseline binds only the declared Host audit file: ${name}`, (context): void => {
    const fixture = routeFixture(context, MODELS_FORWARDED);
    assert.throws(() => captureClaudeRouteBaseline(rebind(fixture), TARGET), expected);
  });

test("the read must use the binding the baseline recorded", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baseline = captureClaudeRouteBaseline(fixture.binding, TARGET);
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  const { path, sha256 } = fixture.binding.collaborationCli;
  for (const rebound of [
    { maxSegmentBytes: 8192 },
    { serviceId: OTHER_SESSION },
    { serviceEpoch: RESTARTED_EPOCH },
    { collaborationCli: { path: "/bin/agent-collaboration", sha256 } },
    { collaborationCli: { path, sha256: "f".repeat(64) } },
  ])
    assert.throws(
      () => readClaudeRouteWindow(baseline, { ...fixture.binding, ...rebound }),
      /differs from the recorded baseline/,
      JSON.stringify(rebound),
    );
  fixture.marker({ hostPid: 9999 });
  assert.throws(
    () => readClaudeRouteWindow(baseline, fixture.binding),
    /does not name the declared/,
  );
});

test("a baseline file over either declared cap is refused before any read", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const recorded = captureClaudeRouteBaseline(fixture.binding, TARGET);
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  // The run plan's caps (64 MiB prefix, 1 MiB segment), one past each in the recorded file.
  for (const overCap of [{ max_prefix_bytes: 67_108_865 }, { max_segment_bytes: 1_048_577 }]) {
    const baselinePath = join(fixture.root, `over-cap-${Object.keys(overCap).join("")}.json`);
    writeFileSync(baselinePath, JSON.stringify({ ...recorded, ...overCap }));
    const plan = observerPlan(fixture, baselinePath);
    assert.throws(() => readPlanClaudeRoute(plan), /too_big|<=/u, JSON.stringify(overCap));
    assert.throws(() => readFileSync(join(plan.evidence, "claude-route-window.json")), /ENOENT/u);
  }
});

test("a route window belongs to one idle-bind Claude plan with its own genuine reply check", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  const route = { kind: "claude-route", baseline: baselinePath, before_thread_read: baselinePath };
  const reply = { kind: "ordinary-reply", prompt: "Compute 1 + 2345678.", completion: "12345679" };
  assert.doesNotThrow(() => observerPlan(fixture, baselinePath));
  for (const [overrides, expected] of [
    [{ checks: [route] }, /its own genuine reply check/],
    [{ checks: [reply, route, route] }, /its own genuine reply check/],
    [{ endpoint_id: "cursor-local" }, /its own genuine reply check/],
    [{ mode: "active-read-only" }, /active-read-only allows only turn-in-progress/],
    [{ checks: [reply, { ...route, baseline: "relative.json" }] }, /absolute path required/],
    [{ checks: [reply, { kind: "claude-route", baseline: baselinePath }] }, /before_thread_read/],
  ] as const)
    assert.throws(
      () => observerPlan(fixture, baselinePath, overrides),
      expected,
      JSON.stringify(overrides),
    );
});
