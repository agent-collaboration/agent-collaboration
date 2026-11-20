// Busy-turn controls driven through ordinary MCP and the Router's public ACP socket.
import { lstatSync, realpathSync } from "node:fs";
import { createConnection } from "node:net";
import { once } from "node:events";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { z } from "zod";
import type { SessionRef } from "../observations/observation-models.ts";
import * as requests from "../transport/journey-requests.ts";
import { baseline, createSession, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import { answered, callTool, record } from "./journey-steps.ts";
import {
  APPLIED,
  control,
  holdWire,
  inspect,
  negotiated,
  observed,
  preparePrompt,
  promptFrame,
  requireAdmission,
  requireFailure,
  requireHeldReply,
  requireOperation,
  startPrompt,
  verifyCompletedPrompt,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

const heldPromptStep = "busy-held-prompt";
type BusyTurn = { target: SessionRef; generation: unknown };
type Prompt = Awaited<ReturnType<typeof preparePrompt>>;
const STEER_MESSAGE =
  "this harness can't take a steer mid-turn; a turn is running; queue the message or cancel the turn";

async function withHeldPrompt(
  inputs: CaseInputs,
  target: SessionRef,
  act: (turn: BusyTurn) => Promise<Prompt>,
): Promise<void> {
  const before = await baseline(inputs, "busy-baseline", target);
  const first = await preparePrompt(inputs, target, heldPromptStep);
  const release = holdWire(inputs, {
    method: "session/prompt",
    sessionId: target.sessionId,
    promptToken: first.token,
    releaseReply: true,
  });
  let candidate: Prompt;
  try {
    requireAdmission(
      await startPrompt(inputs, heldPromptStep, target, first),
      first.operationId,
      target,
    );
    const request = await waitForWire(
      inputs,
      "active prompt request",
      promptFrame(target, first.token),
    );
    const reply = await requireHeldReply(inputs, target, "session/prompt", request.sequence);
    if (reply.frame?.id !== request.frame?.id) throw new Error("the gate held a different reply");
    if ((await inspect(inputs, "busy-running-inspect", target)).state !== "running")
      throw new Error("the Router did not retain a running turn behind the gate");
    candidate = await act({ target, generation: before.result.generation });
  } finally {
    release();
  }
  await verifyCompletedPrompt(
    inputs,
    heldPromptStep,
    target,
    first,
    await waitOperation(inputs, heldPromptStep + "-wait", first.operationId),
  );
  const wire = wireEntries(inputs);
  const second = wire.filter(
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.sessionId === target.sessionId &&
      (promptFrame(target, candidate.token)(entry) || entry.method === "_session/steering"),
  );
  const cancels = wire.filter(
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.sessionId === target.sessionId &&
      entry.method === "session/cancel",
  );
  record(inputs.context, {
    step: "busy-provider-nondispatch",
    target,
    candidate: candidate.operationId,
    secondDispatches: second.length,
    vendorCancels: cancels.length,
  });
  if (second.length || cancels.length)
    throw new Error("the refused or cancelled input reached the provider");
}

async function cancelQueued(inputs: CaseInputs, { target, generation }: BusyTurn): Promise<Prompt> {
  const queued = await preparePrompt(inputs, target, "busy-queued-input");
  const admission = await startPrompt(inputs, "busy-queued-input", target, queued, "queue");
  z.looseObject({ kind: z.literal("queued"), operationId: z.literal(queued.operationId) }).parse(
    answered(admission, "queue admission"),
  );
  const cancelId = requests.newOperationId();
  requireAdmission(
    await callTool(
      inputs.context,
      "busy-queued-cancel",
      "conversation_cancel",
      requests.cancelArguments({
        plan: inputs.plan,
        target,
        operationId: cancelId,
        targetOperationId: queued.operationId,
        generation,
      }),
      { timeoutMs: 15_000 },
    ),
    cancelId,
    target,
  );
  z.looseObject({
    output: z.looseObject({
      kind: z.literal("available"),
      settlement: z.looseObject({
        kind: z.literal("cancelRequested"),
        targetOperationId: z.literal(queued.operationId),
      }),
    }),
  }).parse(
    answered(
      await waitOperation(inputs, "busy-queued-cancel-wait", cancelId),
      "queue cancellation",
    ),
  );
  await requireOperation(
    inputs,
    "busy-queued-cancel",
    target,
    cancelId,
    "conversationCancel",
    APPLIED,
  );
  const ended = await waitOperation(inputs, "busy-queued-terminal", queued.operationId);
  const failure = z
    .object({ kind: z.string(), message: z.literal("not sent: cancelled from the queue") })
    .parse(ended.kind === "refused" ? ended.value : null);
  if (
    requireFailure(ended, queued.operationId, target, failure.kind, failure.message).effect !==
    "none"
  )
    throw new Error("cancelled queue entry shows possible provider dispatch");
  return queued;
}

export async function cancelQueuedCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const target = await createSession(inputs, "queue-create", control(inputs).savedMode);
  await withHeldPrompt(inputs, target, (turn) => cancelQueued(inputs, turn));
  return observed(
    "cancelQueued",
    target,
    "the queued operation settled cancelled without an ACP prompt or interrupt",
  );
}

const rpcEnvelope = z.looseObject({
  jsonrpc: z.literal("2.0"),
  id: z.unknown().optional(),
  method: z.string().optional(),
});

async function refuseSteer(inputs: CaseInputs, target: SessionRef): Promise<Prompt> {
  const steer = await preparePrompt(inputs, target, "busy-explicit-steer");
  const path = join(inputs.plan.serviceDirectory, "codex-acp.sock");
  if (!lstatSync(path).isSocket() || realpathSync(path) !== path)
    throw new Error("the private Router ACP socket is absent or linked");
  const socket = createConnection(path);
  const lines = createInterface({ input: socket, crlfDelay: Infinity });
  const incoming = lines[Symbol.asyncIterator]();
  const timeout = setTimeout(
    () => socket.destroy(new Error("explicit ACP steer receipt timed out")),
    15_000,
  );
  try {
    await once(socket, "connect");
    async function request(id: number, method: string, params: object) {
      const envelope = { jsonrpc: "2.0", id, method, params };
      record(inputs.context, { step: "busy-acp-request", envelope });
      socket.write(JSON.stringify(envelope) + "\n");
      for (;;) {
        const line = await incoming.next();
        if (line.done) throw new Error("ACP socket ended before its correlated reply");
        const reply = rpcEnvelope.parse(JSON.parse(line.value));
        if (reply.id !== id || reply.method !== undefined) continue;
        record(inputs.context, { step: "busy-acp-response", id, reply });
        return reply;
      }
    }
    z.looseObject({ result: z.object({}) }).parse(
      await request(1, "initialize", {
        protocolVersion: 1,
        clientCapabilities: {},
        _meta: {
          router: { actor: requests.conversationActor(inputs.plan.actor) },
          sessionProfile: { version: 1, elements: [] },
        },
      }),
    );
    const reply = await request(2, "_session/steering", {
      sessionId: target.sessionId,
      prompt: [{ type: "text", text: steer.text }],
      _meta: { router: { sessionRef: target } },
    });
    z.looseObject({
      error: z.looseObject({
        code: z.literal(-32601),
        message: z.literal(STEER_MESSAGE),
        data: z.looseObject({ kind: z.literal("cannotSteer") }),
      }),
    }).parse(reply);
    return steer;
  } finally {
    clearTimeout(timeout);
    lines.close();
    socket.destroy();
  }
}

export async function refusedSteerCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const target = await createSession(inputs, "steer-create", control(inputs).savedMode);
  if ((await inspect(inputs, "steer-capability", target)).capabilities?.steer !== false)
    throw new Error("the held-turn control requires a non-steering provider");
  await withHeldPrompt(inputs, target, () => refuseSteer(inputs, target));
  return observed("refusedSteer", target, STEER_MESSAGE);
}
