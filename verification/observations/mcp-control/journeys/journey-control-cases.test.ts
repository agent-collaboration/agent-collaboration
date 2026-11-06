// The negative cases qualify only on their exact typed refusal: HTTP, protocol, schema and
// unrelated refusals, and the right kind at the wrong target or operation, all fail to qualify.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { test } from "node:test";
import type { CaseResult } from "./journey-cases.ts";
import { startStandIn, type StandInOptions } from "../test-support/journey-stand-in.ts";
import { standInPlan } from "../test-support/journey-stand-in-plan.ts";
import { ProviderStoreReadback } from "../evidence/role-readback.ts";
import { runCases } from "../run.ts";
import { StreamingMcpClient } from "../transport/streaming-mcp-client.ts";
import {
  isStaleCancelRefusal,
  isWrongActorRefusal,
  typedRefusal,
} from "./journey-control-cases.ts";
import type { McpCallOutcome } from "../transport/streaming-mcp-client.ts";

const TARGET = { endpoint: { serviceId: "svc", endpointId: "cursor-local" }, sessionId: "s-1" };
const OTHER = { ...TARGET, endpoint: { ...TARGET.endpoint, endpointId: "claude-local" } };
const refused = (value: Readonly<Record<string, unknown>>): McpCallOutcome => ({
  kind: "refused",
  value,
  notifications: [],
});
const notActive = (fields: Readonly<Record<string, unknown>> = {}): McpCallOutcome =>
  refused({
    mcpResult: "error",
    kind: "notFound",
    message: "target provider prompt operation is not active on this binding",
    stage: "validation",
    effect: "none",
    operationId: "op-cancel",
    target: TARGET,
    ...fields,
  });

function refusalOf(outcome: McpCallOutcome): Exclude<ReturnType<typeof typedRefusal>, string> {
  const refusal = typedRefusal(outcome);
  if (typeof refusal === "string") throw new Error(refusal);
  return refusal;
}

test("only typed MCP tool errors count as refusals", () => {
  assert.equal(typeof typedRefusal({ kind: "httpRefusal", status: 422, body: "bad" }), "string");
  assert.equal(typeof typedRefusal({ kind: "protocolRefusal", error: { code: -32602 } }), "string");
  assert.equal(
    typeof typedRefusal({ kind: "unconfirmed", reason: "transport", notifications: [] }),
    "string",
  );
  assert.equal(typeof typedRefusal(refused({ error: "untyped" })), "string");
  assert.equal(typeof typedRefusal(notActive()), "object");
});

test("a stale cancel qualifies only on not-active, validation, no effect, this cancel and target", () => {
  assert.equal(isStaleCancelRefusal(refusalOf(notActive()), "op-cancel", TARGET), true);
  for (const change of [
    { kind: "busy" },
    { stage: "dispatch" },
    { effect: "unknown" },
    { operationId: "op-other" },
    { target: OTHER },
  ])
    assert.equal(
      isStaleCancelRefusal(refusalOf(notActive(change)), "op-cancel", TARGET),
      false,
      JSON.stringify(change),
    );
});

test("a wrong-role change qualifies only on wrongActor at the exact target", () => {
  const wrongActor = refusalOf(
    refused({
      mcpResult: "error",
      kind: "wrongActor",
      message: "only the creator or Approver",
      target: TARGET,
    }),
  );
  assert.equal(isWrongActorRefusal(wrongActor, TARGET), true);
  assert.equal(isWrongActorRefusal(wrongActor, OTHER), false);
  const otherKind = refusalOf(
    refused({ mcpResult: "error", kind: "invalidSetting", message: "x", target: TARGET }),
  );
  assert.equal(isWrongActorRefusal(otherKind, TARGET), false);
});

// loadResume through the real case entry against the scripted Host. The faithful stand-in
// reproduces the first passing controlled run's structure (mcf13): resume settles Resumed with
// history unavailable and a new epoch holding no turn; load settles Loaded and opens a new
// epoch whose baseline replays every earlier exchange in order as historical turns ending
// `stop_reason: {unknown: "replayed"}`, then one new live turn follows its own baseline.
async function loadResume(
  options: StandInOptions,
): Promise<{ readonly result: CaseResult; readonly calls: readonly string[] }> {
  const standIn = await startStandIn(options);
  try {
    const plan = standInPlan(standIn, ["loadResume"]);
    mkdirSync(plan.evidence);
    const store = new ProviderStoreReadback(standIn.store);
    try {
      const context = {
        client: new StreamingMcpClient(standIn.url),
        evidence: plan.evidence,
        deadline: Date.parse(plan.deadline),
      };
      const [result] = await runCases(plan, context, store);
      if (result === undefined) throw new Error("no loadResume result");
      return { result, calls: [...standIn.calls] };
    } finally {
      store.close();
    }
  } finally {
    await standIn.close();
  }
}

test("loadResume qualifies the faithful reattach shapes, including a result-limited replay baseline", async () => {
  for (const readChunk of [200, 3]) {
    const { result } = await loadResume({ readChunk });
    assert.equal(
      result.verdict.kind,
      "qualified",
      `readChunk ${readChunk}: ${JSON.stringify(result.verdict)}`,
    );
  }
});

test("loadResume refuses load history that is not the recorded conversation in order", async () => {
  for (const [loadReplay, reason] of [
    ["none", "the replay holds 0 turns, not the 2 recorded"],
    ["canned", "replayed turn 0 is not the recorded input"],
    ["reordered", "replayed turn 0 is not the recorded input"],
    ["missing", "the replay holds 1 turns, not the 2 recorded"],
    ["foreign", "replayed turn 0 is not the recorded reply"],
    ["liveLooking", "replayed turn 0 does not end as replayed history"],
  ] as const) {
    const { result } = await loadResume({ loadReplay });
    assert.deepEqual(result.verdict, { kind: "failed", reason }, loadReplay);
  }
});

test("loadResume refuses history after resume and a resume settlement that reports history", async () => {
  assert.deepEqual((await loadResume({ resumeReplay: "faithful" })).result.verdict, {
    kind: "failed",
    reason: "resume exposed 1 historical turns; it replays none",
  });
  assert.deepEqual((await loadResume({ resumeHistory: "available" })).result.verdict, {
    kind: "failed",
    reason: "resume reported history available",
  });
});

test("loadResume refuses a resume settlement of another kind", async () => {
  const { result } = await loadResume({ resumeSettlementKind: "closed" });
  assert.deepEqual(result.verdict, {
    kind: "failed",
    reason: "the operation settled closed, not resumed",
  });
});

test("an unretained resume settlement is inconclusive, never a pass", async () => {
  const { result } = await loadResume({ unretainedSettlement: "resumed" });
  assert.deepEqual(result.verdict, {
    kind: "inconclusive",
    reason: "the resumed settlement is outputUnavailable",
  });
});

test("loadResume refuses a load settlement of another kind after a valid resume", async () => {
  const { result, calls } = await loadResume({ loadSettlementKind: "closed" });
  assert.equal(calls.includes("conversation_load"), true);
  assert.deepEqual(result.verdict, {
    kind: "failed",
    reason: "the operation settled closed, not loaded",
  });
});

test("an unretained load settlement after a valid resume is inconclusive, never a pass", async () => {
  const { result, calls } = await loadResume({ unretainedSettlement: "loaded" });
  assert.equal(calls.includes("conversation_load"), true);
  assert.deepEqual(result.verdict, {
    kind: "inconclusive",
    reason: "the loaded settlement is outputUnavailable",
  });
});

test("after load, an old terminal is not new work and an injected input is not isolated", async () => {
  assert.deepEqual((await loadResume({ afterLoadPrompt: "stale" })).result.verdict, {
    kind: "failed",
    reason: "no turn followed the baseline (false completion)",
  });
  assert.deepEqual((await loadResume({ afterLoadPrompt: "injected" })).result.verdict, {
    kind: "inconclusive",
    reason: "another input was accepted on the isolated Session",
  });
});

test("a replay baseline that never completes within its bound is inconclusive and sends no prompt after it", async () => {
  const { result, calls } = await loadResume({ alwaysLimited: true });
  assert.deepEqual(result.verdict, {
    kind: "inconclusive",
    reason: "the replayed history exceeded its declared read bound",
  });
  const afterResume = calls.slice(calls.indexOf("conversation_resume") + 1);
  assert.equal(afterResume.includes("conversation_prompt"), false);
});

test("each segment's submitted text is distinct, so a reordered replay fails even when every reply repeats", async () => {
  assert.equal((await loadResume({ repeatedReply: true })).result.verdict.kind, "qualified");
  for (const [loadReplay, reason] of [
    ["reordered", "replayed turn 0 is not the recorded input"],
    ["foreign", "replayed turn 0 is not the recorded reply"],
  ] as const) {
    const { result } = await loadResume({ repeatedReply: true, loadReplay });
    assert.deepEqual(result.verdict, { kind: "failed", reason }, loadReplay);
  }
});
