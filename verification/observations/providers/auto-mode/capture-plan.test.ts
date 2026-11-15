import assert from "node:assert/strict";
import { test } from "node:test";
import {
  capturePlanSchema,
  checkCaptureScope,
  insideDirectory,
  promptArguments,
  type CapturePlan,
} from "./capture-plan.ts";

function declaredPlan(endpointId = "claude-local"): CapturePlan {
  return capturePlanSchema.parse({
    runtimePlan: "/run/runtime.json",
    target: { endpoint: { serviceId: "service-a", endpointId }, sessionId: "owned-session" },
    caller: {
      endpoint: { serviceId: "service-a", endpointId: "codex-local" },
      sessionId: "actual-caller",
    },
    expectedModel: endpointId === "codex-local" ? "gpt-6-luna" : "claude-haiku-4-5",
    workingDirectory: endpointId === "codex-local" ? "/owned/native-workspace" : null,
    cursorApproval: null,
    evidence: "/run/step-a",
    action: {
      kind: "prompt",
      operationId: "019a9000-0000-7000-8000-000000000001",
      text: "one declared harmless step",
    },
  });
}

test("an unrelated service cannot receive a prompt intended for this run", (): void => {
  const plan = declaredPlan();
  assert.throws((): void => checkCaptureScope(plan, "service-b"), /another service/);
  assert.doesNotThrow((): void => checkCaptureScope(plan, "service-a"));
});

test("Cursor input requires an actual run approval reference; metadata-only capture does not", (): void => {
  const plan = declaredPlan("cursor-local");
  assert.throws((): void => checkCaptureScope(plan, "service-a"), /explicit approval/);
  assert.doesNotThrow((): void =>
    checkCaptureScope({ ...plan, action: { kind: "snapshot" } }, "service-a"),
  );
  assert.doesNotThrow((): void =>
    checkCaptureScope({ ...plan, cursorApproval: "owner approval for run a" }, "service-a"),
  );
});

test("model changes cannot silently consume a different commissioned model", (): void => {
  const plan = declaredPlan("codex-local");
  assert.throws(
    (): void => checkCaptureScope({ ...plan, expectedModel: "another-model" }, "service-a"),
    /only gpt-6-luna/,
  );
  assert.throws(
    (): void => checkCaptureScope({ ...declaredPlan(), expectedModel: "claude-opus" }, "service-a"),
    /only Haiku/,
  );
});

test("provider prompt retains its exact operation ID while native prompt omits it", (): void => {
  const provider = declaredPlan();
  const native = declaredPlan("codex-local");
  assert.equal(promptArguments(provider)["operationId"], "019a9000-0000-7000-8000-000000000001");
  assert.equal(Object.hasOwn(promptArguments(native), "operationId"), false);
  assert.equal(promptArguments(native)["workingDirectory"], "/owned/native-workspace");
  assert.deepEqual(promptArguments(provider)["target"], provider.target);
  assert.throws(
    (): unknown => promptArguments({ ...provider, action: { kind: "snapshot" } }),
    /no prompt/,
  );
});

test("a recorded native route cannot dispatch without an established absolute cwd", (): void => {
  const plan = declaredPlan("codex-local");
  assert.throws(
    (): void => checkCaptureScope({ ...plan, workingDirectory: null }, "service-a"),
    /working directory/,
  );
  assert.equal(
    capturePlanSchema.safeParse({ ...plan, workingDirectory: "relative" }).success,
    false,
  );
  assert.equal(
    capturePlanSchema.safeParse({ ...plan, workingDirectory: undefined }).success,
    false,
  );
  assert.throws(
    (): unknown => promptArguments({ ...plan, workingDirectory: null }),
    /working directory/,
  );
});

test("UUIDv4 input cannot become an operation identity", (): void => {
  const plan = declaredPlan();
  assert.equal(
    capturePlanSchema.safeParse({
      ...plan,
      action: {
        kind: "prompt",
        text: "marker",
        operationId: "019a9000-0000-4000-8000-000000000001",
      },
    }).success,
    false,
  );
});

test("directory containment rejects prefix siblings and dot-dot escapes", (): void => {
  assert.equal(insideDirectory("/owned/session-a", "/owned/session-ab/marker"), false);
  assert.equal(insideDirectory("/owned/session-a", "/owned/session-a/../outside"), false);
  assert.equal(insideDirectory("/owned/session-a", "/owned/session-a/nested/marker"), true);
});
