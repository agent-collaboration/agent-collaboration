// The negative cases qualify only on their exact typed refusal: HTTP, protocol, schema and
// unrelated refusals, and the right kind at the wrong target or operation, all fail to qualify.
import assert from "node:assert/strict";
import { test } from "node:test";
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
