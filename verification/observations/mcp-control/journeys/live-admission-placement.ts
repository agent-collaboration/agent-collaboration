import { z } from "zod";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import {
  sameSession,
  sessionRefSchema,
  type SessionRef,
} from "../observations/observation-models.ts";
import { closeSession, createSession, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import { answered, record } from "./journey-steps.ts";
import {
  APPLIED,
  completePrompt,
  control,
  inspect,
  isAgentReply,
  negotiated,
  notOffered,
  observed,
  preparePrompt,
  promptFrame,
  requireAdmission,
  requireOperation,
  startPrompt,
  verifyCompletedPrompt,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

type Prompt = Awaited<ReturnType<typeof preparePrompt>>;
interface ActiveTurn {
  readonly target: SessionRef;
  readonly prompt: Prompt;
  readonly request: WireEntry;
}
interface PromptWitness {
  readonly request: WireEntry;
  readonly terminal: WireEntry;
}
const queuedAnswer = z.looseObject({
  kind: z.literal("queued"),
  operationId: z.string(),
  target: sessionRefSchema.optional(),
});

function scopedReply(entry: WireEntry, request: WireEntry, method: string): boolean {
  return (
    isAgentReply(method)(entry) &&
    entry.disposition === "forwarded" &&
    entry.sessionId === request.sessionId &&
    entry.frame?.id === request.frame?.id &&
    entry.sequence > request.sequence
  );
}

function promptWitness(inputs: CaseInputs, target: SessionRef, prompt: Prompt): PromptWitness {
  const wire = wireEntries(inputs);
  const requests = wire.filter(promptFrame(target, prompt.token));
  const request = requests[0];
  if (requests.length !== 1 || request === undefined)
    throw new Error("the exact prompt did not dispatch once");
  const terminal = wire.find((entry) => scopedReply(entry, request, "session/prompt"));
  if (terminal === undefined)
    throw new Error("the exact prompt has no forwarded ACP terminal reply");
  return { request, terminal };
}

async function startLongTurn(inputs: CaseInputs, target: SessionRef): Promise<ActiveTurn> {
  const original = await preparePrompt(inputs, target, "live-original");
  const prompt = {
    ...original,
    text: `Write 160 numbered lines, one per line, each saying "Here is one line of this list." Do not summarize or use tools. [${original.token}]`,
  };
  requireAdmission(
    await startPrompt(inputs, "live-original", target, prompt),
    prompt.operationId,
    target,
  );
  const request = await waitForWire(
    inputs,
    "original ACP prompt",
    promptFrame(target, prompt.token),
  );
  return { target, prompt, request };
}

async function finishPrompt(inputs: CaseInputs, turn: ActiveTurn): Promise<PromptWitness> {
  const { target, prompt } = turn;
  const end = await waitOperation(inputs, "live-original-wait", prompt.operationId);
  await verifyCompletedPrompt(inputs, "live-original", target, prompt, end);
  return promptWitness(inputs, target, prompt);
}

async function offerQueued(inputs: CaseInputs, turn: ActiveTurn, number: number): Promise<Prompt> {
  const label = `live-queue-${number}`;
  const prompt = await preparePrompt(inputs, turn.target, label);
  if ((await inspect(inputs, `${label}-before`, turn.target)).state !== "running")
    throw new Error(`${label}: the provider ended before the queue input was offered`);
  const result = queuedAnswer.parse(
    answered(await startPrompt(inputs, label, turn.target, prompt, "queue"), label),
  );
  if (
    result.operationId !== prompt.operationId ||
    (result.target !== undefined && !sameSession(result.target, turn.target))
  )
    throw new Error(`${label}: the queued acknowledgement names another input`);
  const during = wireEntries(inputs);
  const firstEnded = during.some((entry) => scopedReply(entry, turn.request, "session/prompt"));
  const dispatched = during.some(promptFrame(turn.target, prompt.token));
  record(inputs.context, {
    step: "live-queued-ack",
    label,
    operationId: prompt.operationId,
    firstEnded,
    dispatched,
  });
  if (firstEnded || dispatched)
    throw new Error(`${label}: the ACP ordering did not leave the acknowledged input behind A`);
  return prompt;
}

function recordTerminalRows(inputs: CaseInputs, targets: readonly SessionRef[]): void {
  const rows = targets.flatMap((target) => inputs.store.operations(target));
  const nonTerminal = inputs.store.nonTerminalCount();
  record(inputs.context, { step: "live-terminal-rows", targets, rows, nonTerminal });
  if (nonTerminal !== 0 || rows.some((row) => !row.terminal || row.stage !== "terminal"))
    throw new Error("the private store contains a nonterminal operation");
}

export async function liveQueueCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const target = await createSession(inputs, "live-create-a", control(inputs).savedMode);
  const turn = await startLongTurn(inputs, target);
  const count = inputs.plan.endpointId === "cursor-local" ? 1 : 2;
  const queued: Prompt[] = [];
  let previous: PromptWitness;
  try {
    for (let index = 1; index <= count; index++)
      queued.push(await offerQueued(inputs, turn, index));
  } finally {
    previous = await finishPrompt(inputs, turn);
  }
  for (const [index, prompt] of queued.entries()) {
    const label = `live-queue-${index + 1}`;
    const outcome = await waitOperation(inputs, `${label}-wait`, prompt.operationId);
    await verifyCompletedPrompt(inputs, label, target, prompt, outcome);
    const current = promptWitness(inputs, target, prompt);
    record(inputs.context, {
      step: "live-queue-dispatch-order",
      label,
      previousTerminal: previous.terminal.sequence,
      request: current.request.sequence,
      terminal: current.terminal.sequence,
    });
    if (previous.terminal.sequence >= current.request.sequence)
      throw new Error(`${label}: dispatch preceded the previous turn's terminal reply`);
    previous = current;
  }
  recordTerminalRows(inputs, [target]);
  return observed(
    "liveQueue",
    target,
    `${count} queued inputs each dispatched once in order after the original turn`,
  );
}

const steerSettlement = z.looseObject({
  output: z.looseObject({
    kind: z.literal("available"),
    settlement: z.looseObject({
      kind: z.string(),
      target: sessionRefSchema,
      stopReason: z.unknown().optional(),
    }),
  }),
});

async function sendSteer(inputs: CaseInputs, turn: ActiveTurn): Promise<string> {
  const label = "live-steer";
  const { target } = turn;
  const prompt = await preparePrompt(inputs, target, label);
  if ((await inspect(inputs, `${label}-before`, target)).state !== "running")
    throw new Error("the original ended before the explicit steer");
  requireAdmission(
    await startPrompt(inputs, label, target, prompt, "steer"),
    prompt.operationId,
    target,
  );
  const result = steerSettlement.parse(
    answered(await waitOperation(inputs, `${label}-wait`, prompt.operationId), label),
  ).output.settlement;
  if (!sameSession(result.target, target)) throw new Error("steer settled on another Session");
  const wire = wireEntries(inputs);
  const sent = wire.filter(
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.method === "_session/steering" &&
      entry.disposition === "forwarded" &&
      entry.sessionId === target.sessionId &&
      entry.frame?.params?.prompt?.some((block) => block.text?.includes(prompt.token)) === true,
  );
  const request = sent.length === 1 ? sent[0] : undefined;
  const reply =
    request === undefined
      ? undefined
      : wire.find((entry) => scopedReply(entry, request, "_session/steering"));
  const outcome = z.looseObject({ outcome: z.string() }).safeParse(reply?.frame?.result);
  const fallbackPrompts = wire.filter(promptFrame(target, prompt.token));
  record(inputs.context, {
    step: "live-steer-witness",
    target,
    request,
    reply,
    result,
    fallbackSequences: fallbackPrompts.map((frame) => frame.sequence),
  });
  if (!outcome.success || request === undefined || request.sequence <= turn.request.sequence)
    throw new Error("no exact single forwarded steer exchange");
  await requireOperation(inputs, label, target, prompt.operationId, "conversationPrompt", APPLIED);
  if (outcome.data.outcome === "startedNewTurn") return "startedNewTurn";
  if (
    outcome.data.outcome !== "injected" ||
    result.kind !== "steered" ||
    fallbackPrompts.length !== 0
  )
    throw new Error(`steer answered ${outcome.data.outcome} and settled ${result.kind}`);
  return "injected";
}

export async function liveSteerCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const target = await createSession(inputs, "live-create-a", control(inputs).savedMode);
  if ((await inspect(inputs, "live-steer-capability", target)).capabilities?.steer !== true)
    return notOffered("liveSteer", "live capability did not advertise steering");
  const turn = await startLongTurn(inputs, target);
  let outcome: string;
  let end: PromptWitness;
  try {
    outcome = await sendSteer(inputs, turn);
  } finally {
    end = await finishPrompt(inputs, turn);
  }
  const steer = wireEntries(inputs).filter(
    (entry) =>
      entry.method === "_session/steering" &&
      entry.sessionId === target.sessionId &&
      entry.disposition === "forwarded",
  );
  record(inputs.context, {
    step: "live-steer-order",
    steer: steer.map((entry) => entry.sequence),
    originalTerminal: end.terminal.sequence,
  });
  recordTerminalRows(inputs, [target]);
  if (outcome !== "injected")
    return {
      name: "liveSteer",
      target,
      verdict: {
        kind: "inconclusive",
        reason: "the harness reported startedNewTurn for the explicit steer",
      },
    };
  if (steer.length !== 2 || steer.some((entry) => entry.sequence >= end.terminal.sequence))
    throw new Error("the steer exchange was not entirely within the running turn");
  return observed("liveSteer", target, "one explicit steer injected into the same completed turn");
}

export async function liveOtherSessionCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const mode = control(inputs).savedMode;
  const target = await createSession(inputs, "live-create-a", mode);
  const other = await createSession(inputs, "live-create-b", mode);
  if (sameSession(target, other) || (await inspect(inputs, "live-b-idle", other)).state !== "idle")
    throw new Error("the second Session is not a distinct idle target");
  const turn = await startLongTurn(inputs, target);
  let bEnd: WireEntry;
  let aEnd: PromptWitness;
  try {
    if ((await inspect(inputs, "live-a-before-b", target)).state !== "running")
      throw new Error("A finished before the independent prompt could be offered");
    bEnd = (await completePrompt(inputs, "live-b-prompt", other)).terminal;
  } finally {
    aEnd = await finishPrompt(inputs, turn);
  }
  record(inputs.context, {
    step: "live-other-session-order",
    a: target,
    b: other,
    aPrompt: turn.request.sequence,
    bTerminal: bEnd.sequence,
    aTerminal: aEnd.terminal.sequence,
  });
  recordTerminalRows(inputs, [target, other]);
  if (bEnd.sequence >= aEnd.terminal.sequence)
    return {
      name: "liveOtherSession",
      target,
      verdict: { kind: "inconclusive", reason: "B completed after A's running turn ended" },
    };
  return observed("liveOtherSession", target, "B's prompt completed while A was still running");
}

const notSent = z.looseObject({
  operationId: z.string(),
  target: sessionRefSchema,
  message: z.string(),
  effect: z.literal("none"),
});

export async function liveQueueCloseCase(inputs: CaseInputs): Promise<CaseResult> {
  if (!(await negotiated(inputs)).close) return notOffered("liveQueueClose", "session/close");
  const target = await createSession(inputs, "live-create-a", control(inputs).savedMode);
  const turn = await startLongTurn(inputs, target);
  let queued: Prompt;
  try {
    queued = await offerQueued(inputs, turn, 1);
    await closeSession(inputs, target, "live-close");
  } finally {
    const initial = await waitOperation(
      inputs,
      "live-original-after-close",
      turn.prompt.operationId,
    );
    record(inputs.context, { step: "live-close-original-settlement", initial });
  }
  const outcome = await waitOperation(inputs, "live-queued-after-close", queued.operationId);
  const refused = notSent.parse(outcome.kind === "refused" ? outcome.value : null);
  const wire = wireEntries(inputs);
  const queueDispatches = wire.filter(promptFrame(target, queued.token));
  const closeRequest = wire.find(
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.method === "session/close" &&
      entry.sessionId === target.sessionId,
  );
  const closeReply =
    closeRequest === undefined
      ? undefined
      : wire.find((entry) => scopedReply(entry, closeRequest, "session/close"));
  record(inputs.context, {
    step: "live-close-queue-witness",
    target,
    queued: queued.operationId,
    refused,
    queueDispatches,
    closeRequest,
    closeReply,
  });
  if (
    refused.operationId !== queued.operationId ||
    !sameSession(refused.target, target) ||
    !refused.message.startsWith("not sent:") ||
    !refused.message.includes("session closed") ||
    queueDispatches.length !== 0
  )
    throw new Error("the queued input did not settle known unsent for the exact close");
  const queuedStored = inputs.store
    .operations(target)
    .find((row) => row.operationId === queued.operationId);
  if (queuedStored !== undefined)
    await requireOperation(
      inputs,
      "live-queue-closed",
      target,
      queued.operationId,
      "conversationPrompt",
      { effect: "none", reconciliation: "confirmed" },
    );
  else
    record(inputs.context, {
      step: "live-queue-closed-rowless",
      queued: queued.operationId,
      stored: null,
    });
  recordTerminalRows(inputs, [target]);
  return observed(
    "liveQueueClose",
    target,
    "the close settled and the waiting input was confirmed not sent",
  );
}
