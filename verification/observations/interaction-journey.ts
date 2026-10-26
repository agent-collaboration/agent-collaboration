import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { evidenceJson } from "../evidence-json.ts";
import { callbackPrompt } from "./callback-oracles.ts";
import { decideCallback, scopedRecords } from "./callback-decisions.ts";
import { verifyRetained } from "./callback-retained.ts";
import {
  approverFor,
  callerFor,
  journeyRecord,
  loadJourneyPlan,
  operationId,
  type JourneyPlan,
} from "./provider-journey-plan.ts";
import { qualifyBeforeInput, qualifyRetained } from "./provider-journey-qualification.ts";
import { rpcCall } from "./provider-journey-rpc.ts";
import {
  closeSession,
  createSession,
  initializeApprover,
  inspectCreatedSession,
  observeSession,
} from "./provider-session-control.ts";
import { registerGuardIdentifiers } from "./readonly-codex-home.ts";
import { runtimeClock, saveRecord } from "./runtime-plan.ts";
import type { JsonObject } from "./evidence-models.ts";

type RpcOutcome =
  | { readonly kind: "fulfilled"; readonly value: JsonObject }
  | { readonly kind: "rejected"; readonly error: unknown };
function completedValue(result: RpcOutcome): JsonObject {
  if (result.kind === "rejected") throw result.error;
  return result.value;
}
function trackRpc(promise: Promise<JsonObject>): {
  readonly promise: Promise<RpcOutcome>;
  readonly snapshot: () => RpcOutcome | undefined;
} {
  let current: RpcOutcome | undefined;
  const completed = promise
    .then(
      (value): RpcOutcome => ({ kind: "fulfilled", value }),
      (error: unknown): RpcOutcome => ({ kind: "rejected", error }),
    )
    .then((result): RpcOutcome => {
      current = result;
      return result;
    });
  return { promise: completed, snapshot: (): RpcOutcome | undefined => current };
}

async function awaitCallback(input: {
  readonly plan: JourneyPlan;
  readonly kind: string;
  readonly target: JsonObject;
  readonly deadline: number;
  readonly prompted: () => RpcOutcome | undefined;
}): Promise<JsonObject[]> {
  const { plan, kind, target, deadline, prompted } = input;
  while (performance.now() < deadline) {
    const records = await scopedRecords(plan, kind, target);
    if (records.length > 0) return records;
    const unexpected = await scopedRecords(
      plan,
      kind === "question" ? "approval" : "question",
      target,
    );
    if (unexpected.length > 0) {
      saveRecord(join(plan.evidence, "unexpected-pending.json"), unexpected);
      throw new Error("unexpected interaction ordering/tool; no decision");
    }
    const early = prompted();
    if (early !== undefined) {
      saveRecord(join(plan.evidence, "early-prompt-result.json"), completedValue(early));
      throw new Error(`real provider emitted no ${kind}; no retry`);
    }
    await runtimeClock.delay(200);
  }
  const error = new Error(`no real ${kind} within original turn bound`);
  error.name = "TimeoutError";
  throw error;
}

async function promptWithinDeadline(
  promise: Promise<RpcOutcome>,
  deadline: number,
): Promise<JsonObject> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return completedValue(
      await Promise.race([
        promise,
        new Promise<never>((_resolve, reject): void => {
          timer = setTimeout(
            (): void => {
              const error = new Error("original prompt turn bound exceeded");
              error.name = "TimeoutError";
              reject(error);
            },
            Math.max(1000, deadline - performance.now()),
          );
        }),
      ]),
    );
  } finally {
    clearTimeout(timer);
  }
}

export async function driveCallbacks(plan: JourneyPlan): Promise<void> {
  await qualifyBeforeInput(plan);
  const target = journeyRecord(plan, "target-binding");
  const caller = callerFor(plan);
  const actor = { kind: "session", session: approverFor(plan) };
  const marker = join(dirname(plan.evidence), "workspace", "automated-approval-marker");
  if (existsSync(marker)) throw new Error("marker already exists");
  const command = `printf '%s\\n' 'VERIFICATION_MARKER' > ${marker}`;
  if (plan.tools === null) throw new Error("actual provider tools unqualified");
  const prompt = callbackPrompt(plan.tools, command);
  const operation = operationId();
  registerGuardIdentifiers(dirname(plan.evidence), { operationId: operation, target });
  saveRecord(join(plan.evidence, "prompt-intent.json"), {
    operationId: operation,
    target,
    actor,
    command,
    prompt,
    testAutomation: true,
  });
  const observer = trackRpc(observeSession(plan, 120));
  const prompted = trackRpc(
    rpcCall(plan, "conversation_prompt", {
      operationId: operation,
      target,
      requestedBy: caller,
      approver: approverFor(plan),
      message: { kind: "agent", sender: caller, text: prompt },
      timeoutSeconds: 120,
    }),
  );
  const deadline = performance.now() + 120_000;
  try {
    for (const kind of ["question", "approval"]) {
      const records = await awaitCallback({
        plan,
        kind,
        target,
        deadline,
        prompted: prompted.snapshot,
      });
      await decideCallback({ plan, kind, records, actor, command });
    }
    saveRecord(
      join(plan.evidence, "prompt-result.json"),
      await promptWithinDeadline(prompted.promise, deadline),
    );
    saveRecord(
      join(plan.evidence, "operation-terminal.json"),
      await rpcCall(plan, "conversation_operation_show", { operationId: operation }),
    );
    completedValue(await observer.promise);
  } finally {
    await Promise.all([observer.promise, prompted.promise]);
  }
  verifyRetained(plan);
}

export async function main(): Promise<void> {
  const [action, path, ...rest] = process.argv.slice(2);
  if (action === "--help" || action === "-h") {
    console.log(
      "usage: interaction-journey.ts {create,inspect,observe,initialize-approver,close,drive,verify-retained,qualify-retained} PLAN",
    );
    return;
  }
  if (path === undefined || rest.length > 0) throw new Error("action and plan are required");
  const plan = loadJourneyPlan(path);
  switch (action) {
    case "create":
      await createSession(plan);
      return;
    case "inspect":
      await inspectCreatedSession(plan);
      return;
    case "observe":
      await observeSession(plan);
      return;
    case "initialize-approver":
      await initializeApprover(plan);
      return;
    case "close":
      await closeSession(plan);
      return;
    case "drive":
      await driveCallbacks(plan);
      return;
    case "verify-retained":
      verifyRetained(plan);
      return;
    case "qualify-retained":
      console.log(evidenceJson(qualifyRetained(plan)));
      return;
    default:
      throw new Error("unknown callback journey action");
  }
}
if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
