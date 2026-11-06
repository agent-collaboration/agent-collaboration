// Real case wiring and MCP reads against the existing stand-in; no real provider input.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { startStandIn, type StandInOptions } from "../test-support/journey-stand-in.ts";
import { standInPlan } from "../test-support/journey-stand-in-plan.ts";
import { ProviderStoreReadback } from "../evidence/role-readback.ts";
import { StreamingMcpClient } from "../transport/streaming-mcp-client.ts";
import { runCases } from "../run.ts";
import type { CaseResult } from "./journey-cases.ts";

const MODEL = "claude-haiku-5-5";

async function runModelCase(options: StandInOptions): Promise<{
  readonly result: CaseResult;
  readonly calls: readonly string[];
  readonly createModel: unknown;
  readonly evidence: string;
}> {
  const standIn = await startStandIn(options);
  try {
    const plan = standInPlan(standIn, ["loadResume"], { peer: { kind: "provider", model: MODEL } });
    mkdirSync(plan.evidence);
    const store = new ProviderStoreReadback(standIn.store);
    try {
      const [result] = await runCases(
        plan,
        {
          client: new StreamingMcpClient(standIn.url),
          evidence: plan.evidence,
          deadline: Date.parse(plan.deadline),
        },
        store,
      );
      if (result === undefined) throw new Error("missing result");
      return {
        result,
        calls: [...standIn.calls],
        createModel: standIn.requests.find((request) => request.name === "conversation_create")
          ?.arguments["model"],
        evidence: readFileSync(join(plan.evidence, "exchanges.jsonl"), "utf8"),
      };
    } finally {
      store.close();
    }
  } finally {
    await standIn.close();
  }
}

test("provider admission requires a model before any MCP effect; controlled plans stay unchanged", async () => {
  const standIn = await startStandIn();
  try {
    assert.throws(() => standInPlan(standIn, ["lifecycle"], { peer: { kind: "provider" } }));
    const controlled = standInPlan(standIn, ["lifecycle"]);
    assert.equal(controlled.peer.kind, "controlled");
    assert.equal("model" in controlled.peer, false);
    assert.deepEqual(standIn.calls, []);
  } finally {
    await standIn.close();
  }
});

test("provider create sends its declared model and every prompt has a recorded inspect", async () => {
  const observed = await runModelCase({});
  assert.equal(observed.result.verdict.kind, "qualified");
  assert.equal(observed.createModel, MODEL);
  assert.equal(observed.calls.filter((name) => name === "provider_session_inspect").length, 3);
  for (const [index, name] of observed.calls.entries()) {
    if (name === "conversation_prompt")
      assert.equal(observed.calls[index - 1], "provider_session_inspect");
  }
  assert.equal(observed.evidence.split('"tool":"provider_session_inspect"').length - 1, 3);
});

for (const [label, options, prompts, reason] of [
  ["notApplied", { createNotApplied: true }, 0, "creation did not apply all declared settings"],
  [
    "createdWithoutSettings",
    { createdWithoutSettings: true },
    0,
    "creation did not apply all declared settings",
  ],
  [
    "model changed after resume",
    { modelAfterResume: "other-model" },
    1,
    "current model other-model differs from declared model claude-haiku-5-5",
  ],
  [
    "model changed after load",
    { modelAfterLoad: "other-model" },
    2,
    "current model other-model differs from declared model claude-haiku-5-5",
  ],
  [
    "wrong initial model",
    { currentModel: "other-model" },
    0,
    "current model other-model differs from declared model claude-haiku-5-5",
  ],
] as const) {
  test(`provider stops before input: ${label}`, async () => {
    const observed = await runModelCase(options);
    assert.equal(observed.result.verdict.kind, "failed");
    if (observed.result.verdict.kind !== "failed") throw new Error("expected failure");
    assert.equal(
      observed.result.verdict.reason.includes(reason),
      true,
      observed.result.verdict.reason,
    );
    assert.equal(observed.calls.filter((name) => name === "conversation_prompt").length, prompts);
  });
}
