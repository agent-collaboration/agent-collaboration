import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { readPlanClaudeRoute } from "./acp-journey-observation.ts";
import { captureClaudeRouteBaseline } from "./claude-route-window.ts";
import {
  ACCOUNT_HASH,
  COMPLETION,
  FORWARDED_CLAUDE,
  INVENTORY,
  MODELS_FORWARDED,
  OTHER_SESSION,
  PRIOR_TURN,
  PROMPT,
  RESTARTED_EPOCH,
  SECRET,
  SESSION,
  TARGET,
  hostedRow,
  inventoryPage,
  observerPlan,
  routeFixture,
  sessionThread,
  type RouteFixture,
} from "./claude-route-fixture.ts";

// The managed single-Session isolation around the Claude route window, through the real
// pre-Enter baseline capture and the real observer read, each against the controlled CLI
// stand-in: exactly two exact reads, categorical failures, and no raw output retained.
test("the observer reads the window only for its own Host and Session, retaining no raw rows", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  writeFileSync(baselinePath, JSON.stringify(captureClaudeRouteBaseline(fixture.binding, TARGET)));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  const plan = observerPlan(fixture, baselinePath);
  const route = readPlanClaudeRoute(plan);
  assert.equal(route?.window.rows, 1);
  assert.deepEqual(route?.inventoryAfter, INVENTORY);
  assert.deepEqual(
    route?.historyBefore.turns.map((turn) => turn.id),
    ["turn-1"],
  );
  // Exactly two inventory reads in total, the pre-Enter one and the post-reply one, each the
  // exact stored/all/any one-page request with no query or cursor.
  const exact = [
    "sessions",
    "list",
    "--endpoint",
    "claude-local",
    "--view",
    "stored",
    "--source",
    "all",
    "--any",
    "--page-size",
    "100",
    "--service-directory",
    fixture.serviceDirectory,
    "--json",
  ];
  assert.deepEqual(fixture.inventoryCalls(), [exact, exact]);
  const retained = readFileSync(join(plan.evidence, "claude-route-window.json"), "utf8");
  const baselineCopy = readFileSync(join(plan.evidence, "claude-route-baseline.json"), "utf8");
  for (const text of [retained, baselineCopy])
    for (const leaked of [
      ACCOUNT_HASH,
      "local_proxy_request",
      "policy-sentinel",
      "approver-sentinel",
      "creator-sentinel",
      "hello",
      COMPLETION,
    ])
      assert.equal(text.includes(leaked), false, leaked);
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

test("the audit segment is frozen before the final inventory closes its observation window", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const cli = fixture.binding.collaborationCli.path;
  // This controlled CLI appends a row while answering. Its first row precedes the baseline;
  // its second follows the reply readback and must fall outside the already-frozen segment.
  appendFileSync(
    cli,
    `\nrequire("node:fs").appendFileSync(${JSON.stringify(fixture.auditFile)}, ${JSON.stringify(FORWARDED_CLAUDE)});\n`,
  );
  const bound = {
    ...fixture,
    binding: {
      ...fixture.binding,
      collaborationCli: {
        path: cli,
        sha256: createHash("sha256").update(readFileSync(cli)).digest("hex"),
      },
    },
  };
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  writeFileSync(baselinePath, JSON.stringify(captureClaudeRouteBaseline(bound.binding, TARGET)));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);

  const route = readPlanClaudeRoute(observerPlan(bound, baselinePath));

  assert.equal(route?.window.rows, 1);
  assert.equal(route?.window.categories[0]?.count, 1);
  assert.equal(bound.inventoryCalls().length, 2);
});

// Each wrong inventory read, set up on the fixture before the read it targets. Errors are
// categorical: a secret-shaped CLI output never reaches the message.
const INVENTORY_FAILURES: ReadonlyArray<[string, (fixture: RouteFixture) => void, RegExp]> = [
  [
    "another idle managed Session",
    (fixture) => {
      fixture.inventory(
        inventoryPage([hostedRow(SESSION, "idle"), hostedRow(OTHER_SESSION, "idle")]),
      );
    },
    /holds 2 Sessions/,
  ],
  [
    "another closed managed Session",
    (fixture) => {
      fixture.inventory(
        inventoryPage([hostedRow(SESSION, "idle"), hostedRow(OTHER_SESSION, "closed")]),
      );
    },
    /holds 2 Sessions/,
  ],
  [
    "another unloaded managed Session",
    (fixture) => {
      fixture.inventory(
        inventoryPage([hostedRow(OTHER_SESSION, "unloaded"), hostedRow(SESSION, "idle")]),
      );
    },
    /holds 2 Sessions/,
  ],
  [
    "the target running",
    (fixture) => {
      fixture.inventory(inventoryPage([hostedRow(SESSION, "running")]));
    },
    /not idle/,
  ],
  [
    "a failing CLI that prints a secret",
    (fixture) => {
      fixture.inventory(`${SECRET}\n`, 3);
    },
    /failed with exit 3/,
  ],
  [
    "a secret instead of the page",
    (fixture) => {
      fixture.inventory(`${SECRET}\n`);
    },
    /malformed or incomplete/,
  ],
  [
    "the same service restarted before the read",
    (fixture) => {
      fixture.restart();
    },
    /restarted or replaced/,
  ],
  [
    "the same service restarted while the CLI answered",
    (fixture) => {
      fixture.inventory(inventoryPage([hostedRow(SESSION, "idle")]), 0, RESTARTED_EPOCH);
    },
    /restarted or replaced/,
  ],
  [
    "a changed collaboration CLI",
    (fixture) => {
      appendFileSync(fixture.binding.collaborationCli.path, "// changed\n");
    },
    /CLI changed/,
  ],
];
const categorical =
  (expected: RegExp) =>
  (error: unknown): boolean =>
    error instanceof Error && expected.test(error.message) && !error.message.includes(SECRET);

for (const [name, mutate, expected] of INVENTORY_FAILURES)
  test(`the pre-Enter inventory refuses the baseline: ${name}`, (context): void => {
    const fixture = routeFixture(context, MODELS_FORWARDED);
    mutate(fixture);
    assert.throws(() => captureClaudeRouteBaseline(fixture.binding, TARGET), categorical(expected));
  });

for (const [name, mutate, expected] of INVENTORY_FAILURES)
  test(`the post-reply inventory refuses the window: ${name}`, (context): void => {
    const fixture = routeFixture(context, MODELS_FORWARDED);
    const baselinePath = join(fixture.root, "claude-route-baseline.json");
    writeFileSync(
      baselinePath,
      JSON.stringify(captureClaudeRouteBaseline(fixture.binding, TARGET)),
    );
    appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
    const plan = observerPlan(fixture, baselinePath);
    mutate(fixture);
    assert.throws(() => readPlanClaudeRoute(plan), categorical(expected));
    assert.throws(() => readFileSync(join(plan.evidence, "claude-route-window.json")), /ENOENT/u);
  });

test("the route reads use only the normal build's collaboration CLI", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  writeFileSync(baselinePath, JSON.stringify(captureClaudeRouteBaseline(fixture.binding, TARGET)));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  // Each case writes its own manifest, so no case's binding replaces another's before it runs.
  const manifestNaming = (name: string, cli: unknown): string => {
    const otherManifest = join(fixture.root, `${name}-build-manifest.json`);
    writeFileSync(
      otherManifest,
      JSON.stringify({
        source_commit: "head",
        source_tree: "tree",
        dirty: false,
        test_guard_features: [],
        binaries: cli === null ? {} : { "agent-collaboration": cli },
      }),
    );
    return otherManifest;
  };
  const { path, sha256 } = fixture.binding.collaborationCli;
  for (const overrides of [
    { collaboration_cli: "/bin/agent-collaboration" },
    { build_manifest: manifestNaming("wrong-sha", { path, sha256: "f".repeat(64) }) },
    { build_manifest: manifestNaming("wrong-path", { path: "/bin/agent-collaboration", sha256 }) },
    { build_manifest: manifestNaming("no-cli", null) },
  ]) {
    const plan = observerPlan(fixture, baselinePath, overrides);
    assert.throws(
      () => readPlanClaudeRoute(plan),
      /another collaboration CLI than the normal build/,
      JSON.stringify(overrides),
    );
  }
  // Only the pre-Enter read ran: a refused binding never reaches the post-reply CLI read.
  assert.equal(fixture.inventoryCalls().length, 1);
});

test("the observer refuses a retained before-state other than the declared idle Session at its cwd", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const baselinePath = join(fixture.root, "claude-route-baseline.json");
  writeFileSync(baselinePath, JSON.stringify(captureClaudeRouteBaseline(fixture.binding, TARGET)));
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  const reply = { kind: "ordinary-reply", prompt: PROMPT, completion: COMPLETION };
  const idle = sessionThread([PRIOR_TURN]);
  for (const [name, thread] of [
    ["another Session", { ...idle, id: OTHER_SESSION }],
    ["another cwd", { ...idle, cwd: "/work/elsewhere" }],
    ["an active Session", { ...idle, status: { type: "active" } }],
  ] as const) {
    const beforeThreadRead = join(fixture.root, `before-${name.replaceAll(" ", "-")}.json`);
    writeFileSync(beforeThreadRead, JSON.stringify({ id: 2, result: { thread } }));
    const route = {
      kind: "claude-route",
      baseline: baselinePath,
      before_thread_read: beforeThreadRead,
    };
    const plan = observerPlan(fixture, baselinePath, { checks: [reply, route] });
    assert.throws(() => readPlanClaudeRoute(plan), /retained before-state/, name);
  }
  // Refused before the post-reply read: only the pre-Enter inventory read ran.
  assert.equal(fixture.inventoryCalls().length, 1);
});

test("a retained baseline whose inventory names another service or Session is refused", (context): void => {
  const fixture = routeFixture(context, MODELS_FORWARDED);
  const recorded = captureClaudeRouteBaseline(fixture.binding, TARGET);
  appendFileSync(fixture.auditFile, FORWARDED_CLAUDE);
  const foreign = "019a0000-0000-7000-8000-0000000000bb";
  for (const [name, baseline, expected] of [
    [
      "inventory for another service",
      { ...recorded, inventory_before: { ...recorded.inventory_before, service_id: foreign } },
      /names another service or Session than its baseline/,
    ],
    [
      "inventory for another Session",
      {
        ...recorded,
        inventory_before: { ...recorded.inventory_before, session_id: OTHER_SESSION },
      },
      /names another service or Session than its baseline/,
    ],
    [
      "baseline and inventory both moved to another service",
      {
        ...recorded,
        service_id: foreign,
        inventory_before: { ...recorded.inventory_before, service_id: foreign },
      },
      /restarted or replaced/,
    ],
    [
      "another service epoch",
      { ...recorded, service_epoch: RESTARTED_EPOCH },
      /restarted or replaced/,
    ],
  ] as const) {
    const baselinePath = join(fixture.root, `retained-${name.replaceAll(" ", "-")}.json`);
    writeFileSync(baselinePath, JSON.stringify(baseline));
    assert.throws(() => readPlanClaudeRoute(observerPlan(fixture, baselinePath)), expected, name);
  }
});
