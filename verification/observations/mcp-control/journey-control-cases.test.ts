// The negative cases qualify only on their exact typed refusal: HTTP, protocol, schema and
// unrelated refusals, and the right kind at the wrong target or operation, all fail to qualify.
import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { test } from "node:test";
import type { ControlPlan } from "./control-plan.ts";
import type { CaseResult } from "./journey-cases.ts";
import { standInPlan, startStandIn, type StandInOptions } from "./journey-stand-in.ts";
import { ProviderStoreReadback } from "./role-readback.ts";
import { runCases } from "./run.ts";
import { StreamingMcpClient } from "./streaming-mcp-client.ts";
import {
  isStaleCancelRefusal,
  isWrongActorRefusal,
  typedRefusal,
} from "./journey-control-cases.ts";
import type { McpCallOutcome } from "./streaming-mcp-client.ts";

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
    const plan = standInPlan(standIn, ["loadResume"]) as unknown as ControlPlan;
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
  for (const loadReplay of [
    "none",
    "canned",
    "reordered",
    "missing",
    "foreign",
    "liveLooking",
  ] as const) {
    const { result } = await loadResume({ loadReplay });
    assert.equal(result.verdict.kind, "failed", loadReplay);
  }
});

test("loadResume refuses history after resume and a resume settlement that reports history", async () => {
  assert.equal((await loadResume({ resumeReplay: "faithful" })).result.verdict.kind, "failed");
  assert.equal((await loadResume({ resumeHistory: "available" })).result.verdict.kind, "failed");
});

test("loadResume refuses a reattach settled as another kind and an unretained settlement", async () => {
  assert.equal(
    (await loadResume({ reattachSettlementKind: "closed" })).result.verdict.kind,
    "failed",
  );
});

test("after load, an old terminal is not new work and an injected input is not isolated", async () => {
  assert.equal((await loadResume({ afterLoadPrompt: "stale" })).result.verdict.kind, "failed");
  assert.equal(
    (await loadResume({ afterLoadPrompt: "injected" })).result.verdict.kind,
    "inconclusive",
  );
});

test("a replay baseline that never completes within its bound is inconclusive and sends no prompt after it", async () => {
  const { result, calls } = await loadResume({ alwaysLimited: true });
  assert.equal(result.verdict.kind, "inconclusive", JSON.stringify(result.verdict));
  const afterResume = calls.slice(calls.indexOf("conversation_resume") + 1);
  assert.equal(afterResume.includes("conversation_prompt"), false);
});

test("each segment's submitted text is distinct, so a reordered replay fails even when every reply repeats", async () => {
  assert.equal((await loadResume({ repeatedReply: true })).result.verdict.kind, "qualified");
  for (const loadReplay of ["reordered", "foreign"] as const) {
    const { result } = await loadResume({ repeatedReply: true, loadReplay });
    assert.equal(result.verdict.kind, "failed", loadReplay);
  }
});
