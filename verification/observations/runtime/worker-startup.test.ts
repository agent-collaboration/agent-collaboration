import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test, type TestContext } from "node:test";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { readObservedJson } from "../shared/evidence-models.ts";
import {
  requireWorkerSettlement,
  runWorkerStartup,
  workerStartupPlanSchema,
  writeFixtureWrapper,
} from "./worker-startup.ts";
import { decode } from "../../exact-json.ts";
import { runtimeClock } from "./runtime-plan.ts";

test("worker allocation expiry reads the injected verification clock before any process starts", async (context: TestContext): Promise<void> => {
  const wallNow = context.mock.method(runtimeClock, "wallNow", (): number =>
    Date.parse("2100-01-01T00:00:00Z"),
  );
  const plan = workerStartupPlanSchema.parse({
    repository: "/not-a-worker-repository",
    source_head: "a".repeat(40),
    qualification: "/qualification",
    run_directory: "/private/tmp/run",
    evidence: "/private/tmp/evidence",
    router_port: 48121,
    mcp_port: 48122,
    expected_version: "fixture",
    expected_workers: ["schedules"],
    deadline: "2099-01-01T00:00:00Z",
  });
  await assert.rejects(runWorkerStartup(plan), /expired worker observation allocation/);
  assert.equal(wallNow.mock.callCount(), 1);
});

test("the owned executable wrapper runs only the repository startup fixture and preserves quoted paths", (): void => {
  const root = createFixtureDirectory("worker-wrapper");
  try {
    const wrapper = join(root, "fixture wrapper.mjs");
    const receipt = join(root, "receipt 'quoted'.json");
    const fixture = new URL("../../fixtures/provider-startup-idle-agent.ts", import.meta.url)
      .pathname;
    writeFixtureWrapper({ wrapper, fixture, receipt });
    const input = `${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: 1, clientCapabilities: {} } })}\n`;
    const result = spawnSync(wrapper, [], { input, encoding: "utf8", timeout: 5000 });
    assert.equal(result.status, 0, result.stderr);
    const observed = readObservedJson(readFileSync(receipt));
    assert.equal(observed.initialized, true);
    assert.equal(observed.closed, true);
    assert.equal(observed.additionalRequests, 0n);
    assert.match(result.stdout, /worker-startup-fixture/);
    const original = readFileSync(wrapper);
    assert.throws(() => writeFixtureWrapper({ wrapper, fixture, receipt }), /EEXIST/);
    assert.equal(readFileSync(wrapper).equals(original), true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a worker startup plan excludes protected ports and an empty claimed inventory", (): void => {
  const plan = {
    repository: "/declared",
    source_head: "a".repeat(40),
    qualification: "/qualification",
    run_directory: "/private/tmp/run",
    evidence: "/private/tmp/evidence",
    router_port: 48121,
    mcp_port: 48122,
    expected_version: "fixture",
    expected_workers: ["schedules"],
    deadline: "2026-11-08T23:00:00Z",
  };
  assert.equal(workerStartupPlanSchema.parse(plan).idle_milliseconds, 5000);
  const fromJson = workerStartupPlanSchema.parse(
    decode(JSON.stringify({ ...plan, idle_milliseconds: 5000 })),
  );
  assert.equal(fromJson.router_port, plan.router_port);
  assert.equal(fromJson.mcp_port, plan.mcp_port);
  assert.equal(fromJson.idle_milliseconds, 5000);
  assert.equal(workerStartupPlanSchema.safeParse({ ...plan, router_port: 19741 }).success, false);
  assert.equal(workerStartupPlanSchema.safeParse({ ...plan, expected_workers: [] }).success, false);
  assert.equal(
    workerStartupPlanSchema.safeParse({ ...plan, run_directory: `/private/tmp/${"x".repeat(30)}` })
      .success,
    false,
  );
});

test("Router signal termination requires its exact recorded SIGTERM and complete settlement", (): void => {
  const settled = { exit: -15, signals: [{ pid: 1234, signal: 15 }], errors: [], remaining: {} };
  assert.doesNotThrow(() => requireWorkerSettlement(settled, "router", 1234));
  assert.throws(() => requireWorkerSettlement(settled, "host", 1234));
  assert.throws(() => requireWorkerSettlement(settled, "router", 4321));
  assert.throws(() => requireWorkerSettlement({ ...settled, signals: [] }, "router", 1234));
  assert.throws(() => requireWorkerSettlement({ ...settled, exit: -9 }, "router", 1234));
  assert.throws(() =>
    requireWorkerSettlement({ ...settled, remaining: { "1234": "unverified" } }, "router", 1234),
  );
  assert.throws(() =>
    requireWorkerSettlement(
      { ...settled, errors: [{ error: "identity changed" }] },
      "router",
      1234,
    ),
  );
  assert.doesNotThrow(() => requireWorkerSettlement({ ...settled, exit: 0 }, "host", 1234));
});
