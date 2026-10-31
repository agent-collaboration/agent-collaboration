import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import {
  appendFileSync,
  chmodSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  symlinkSync,
  truncateSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { journeyObservationPlanSchema, type JourneyCheckResult } from "../acp-journey-models.ts";
import { readPlanClaudeRoute } from "../acp-journey-observation.ts";
import { checkClaudeRoute } from "../acp-journey-oracle.ts";
import {
  captureClaudeRouteBaseline,
  readClaudeRouteWindow,
  type ClaudeRouteBinding,
  type ClaudeRouteWindow,
} from "../claude-route-window.ts";

// Literal rows as the Router serializes AuditEvent (codex-router-core/src/audit.rs:83-98): a
// forwarded Claude response (http_affinity.rs:128-143), a rejected Claude local token
// (http_affinity.rs:110-125 via protocol_handler.rs:127-191) and Codex's own models traffic.
const ACCOUNT_HASH = "acct_5f3a9c0d";
const FORWARDED_CLAUDE = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"${ACCOUNT_HASH}"}\n`;
const REJECTED_CLAUDE = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"wrong","outcome":"rejected","decision_reason":"local_auth_wrong","response_commit_state":"not_committed","error_class":"local_auth"}\n`;
const MODELS_FORWARDED = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"models","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"acct_openai_77"}\n`;

const SESSION = "e4aa7dc9-8c96-4a78-8bcc-c87356410348";
const PORT = 61613;
const TARGET = {
  afterScene: "prompt-composed",
  providerSessionId: SESSION,
  nativeThreadId: SESSION,
} as const;

interface RouteFixture {
  readonly root: string;
  readonly runDirectory: string;
  readonly serviceDirectory: string;
  readonly auditFile: string;
  readonly binding: ClaudeRouteBinding;
  readonly marker: (overrides: Record<string, unknown>) => void;
}

/** A real owner-private Host run directory with the Host's own marker and the service listener. */
function routeFixture(context: TestContext, auditPrefix: string | null): RouteFixture {
  const root = realpathSync(createFixtureDirectory("claude-route"));
  context.after(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const runDirectory = join(root, "run");
  const serviceDirectory = join(root, "router", "agent-communication");
  mkdirSync(runDirectory, { mode: 0o700 });
  mkdirSync(serviceDirectory, { recursive: true, mode: 0o700 });
  writeFileSync(
    join(serviceDirectory, "service.json"),
    JSON.stringify({
      serviceId: "c4007a11-4c33-43a9-b44d-2a5ff96dd211",
      routerProxyEndpoint: `127.0.0.1:${PORT}`,
    }),
  );
  const marker = (overrides: Record<string, unknown>): void => {
    writeFileSync(
      join(runDirectory, "debug-host-context.json"),
      JSON.stringify({
        kind: "debugHostPrepared",
        runDirectory,
        serviceDirectory,
        workspace: join(runDirectory, "agent-workspace"),
        profile: "codex-router-debug",
        model: "gpt-5.6-luna",
        port: PORT,
        hostPid: 5446,
        routerRoot: join(root, "router"),
        claudeProvider: { executable: "/bin/claude-agent-acp", arguments: [] },
        cursorProvider: null,
        peerRegistryDirectory: null,
        ...overrides,
      }),
      { mode: 0o600 },
    );
  };
  marker({});
  const auditFile = join(runDirectory, "router-audit.jsonl");
  if (auditPrefix !== null) writeFileSync(auditFile, auditPrefix, { mode: 0o600 });
  return {
    root,
    runDirectory,
    serviceDirectory,
    auditFile,
    marker,
    binding: {
      hostRunDirectory: runDirectory,
      hostPid: 5446,
      serviceDirectory,
      maxPrefixBytes: 65_536,
      maxSegmentBytes: 4096,
    },
  };
}

function routeResult(window: ClaudeRouteWindow): JourneyCheckResult {
  return checkClaudeRoute({ claudeRoute: window });
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
  assert.throws(
    () => readClaudeRouteWindow(baseline, { ...fixture.binding, maxSegmentBytes: 8192 }),
    /differs from the recorded baseline/,
  );
  fixture.marker({ hostPid: 9999 });
  assert.throws(
    () => readClaudeRouteWindow(baseline, fixture.binding),
    /does not name the declared/,
  );
});

function observerPlan(
  fixture: RouteFixture,
  baselinePath: string,
  overrides: Record<string, unknown> = {},
) {
  const evidence = join(fixture.root, `evidence-${Math.random().toString(16).slice(2)}`);
  mkdirSync(evidence, { mode: 0o700 });
  return journeyObservationPlanSchema.parse({
    repository: "/repo",
    source_head: "head",
    build_manifest: "/repo/manifest.json",
    host_pid: 5446,
    host_binary: "/bin/host",
    host_sha256: "h",
    router_binary: "/bin/router",
    router_sha256: "r",
    codex_cli: "/bin/codex",
    codex_sha256: "c",
    collaboration_cli: "/bin/agent-collaboration",
    service_directory: fixture.serviceDirectory,
    endpoint_id: "claude-local",
    session_id: SESSION,
    native_thread_id: SESSION,
    evidence,
    cwd: "/work/provider",
    mode: "idle-bind",
    checks: [
      {
        kind: "ordinary-reply",
        prompt: "Compute 1 + 2345678. Reply with only the integer.",
        completion: "12345679",
      },
      { kind: "claude-route", baseline: baselinePath },
    ],
    ...overrides,
  });
}

test("the observer reads the window only for its own Host and Session, retaining no raw rows", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  writeFileSync(baselinePath, JSON.stringify(captureClaudeRouteBaseline(fixture.binding, TARGET)));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  const plan = observerPlan(fixture, baselinePath);
  const window = readPlanClaudeRoute(plan);
  assert.equal(window?.rows, 1);
  const retained = readFileSync(join(plan.evidence, "claude-route-window.json"), "utf8");
  assert.equal(retained.includes(ACCOUNT_HASH), false);
  assert.equal(retained.includes("local_proxy_request"), false);
  for (const overrides of [
    { host_pid: 5447 },
    { session_id: "other-session" },
    { native_thread_id: "019a0000-0000-7000-8000-000000000001" },
  ])
    assert.throws(
      () => readPlanClaudeRoute(observerPlan(fixture, baselinePath, overrides)),
      /another Host or Session/,
      JSON.stringify(overrides),
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
  const route = { kind: "claude-route", baseline: baselinePath };
  const reply = { kind: "ordinary-reply", prompt: "Compute 1 + 2345678.", completion: "12345679" };
  assert.doesNotThrow(() => observerPlan(fixture, baselinePath));
  for (const [overrides, expected] of [
    [{ checks: [route] }, /its own genuine reply check/],
    [{ checks: [reply, route, route] }, /its own genuine reply check/],
    [{ endpoint_id: "cursor-local" }, /its own genuine reply check/],
    [{ mode: "active-read-only" }, /active-read-only allows only turn-in-progress/],
    [
      { checks: [reply, { kind: "claude-route", baseline: "relative.json" }] },
      /absolute path required/,
    ],
  ] as const)
    assert.throws(
      () => observerPlan(fixture, baselinePath, overrides),
      expected,
      JSON.stringify(overrides),
    );
});
