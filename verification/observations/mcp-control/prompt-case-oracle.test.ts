// Sensitivity of the isolated prompt case: each wrong build below must fail or stay
// inconclusive, and the allowed alternatives (a bounded ending with continuationGap true, an
// empty baseline) must still qualify. The stand-in data is shaped like the Router's real
// bounded results; it proves the oracle's mechanics only, never a Host or provider path.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  boundedObservationResultSchema,
  observationNotificationSchema,
  type SessionRef,
} from "./observation-models.ts";
import { foldObservationReads, type ObservationRead } from "./observation-ledger.ts";
import { decidePromptCase, type PromptCaseEvidence } from "./prompt-case-oracle.ts";

const TARGET: SessionRef = {
  endpoint: { serviceId: "svc-1", endpointId: "cursor-local" },
  sessionId: "session-1",
};
const OPERATION = "op-prompt-1";
type RawEvent = Readonly<Record<string, unknown>>;

function numbered(sequence: number, event: RawEvent): RawEvent {
  return { sequence: BigInt(sequence), event };
}
function agentItem(text: string): RawEvent {
  return { itemId: "item-1", kind: { kind: "agentMessage" }, text };
}
function promptTurn(first: number, turnId: string, inputId: string): readonly RawEvent[] {
  return [
    numbered(first, { kind: "turnStarted", turnId, inputId }),
    numbered(first + 1, { kind: "inputAccepted", inputId, turnId }),
    numbered(first + 2, { kind: "itemStarted", item: agentItem("before") }),
    numbered(first + 3, { kind: "itemUpdated", item: agentItem("beforeafter") }),
    numbered(first + 4, { kind: "itemCompleted", itemId: "item-1" }),
    numbered(first + 5, {
      kind: "turnEnded",
      turnId,
      outcome: { kind: "ended", stopReason: "endTurn", localCause: null },
    }),
  ];
}
const PROMPT_TURN = promptTurn(3, "turn-1", "input-1");
const BASELINE: readonly RawEvent[] = [
  numbered(1, { kind: "stateChanged", state: { kind: "idle" } }),
  numbered(2, { kind: "settingsChanged", settings: {} }),
];

interface ReadSpec {
  readonly after: number | null;
  readonly events: readonly RawEvent[];
  readonly epoch?: number;
  readonly requestEpoch?: number | null;
  readonly target?: SessionRef;
  readonly endReason?: string;
  readonly notifications?: readonly RawEvent[];
}

function read(spec: ReadSpec, isBaseline: boolean): ObservationRead {
  const epoch = BigInt(spec.epoch ?? 7);
  const result = boundedObservationResultSchema.parse({
    target: spec.target ?? TARGET,
    generation: { serviceEpoch: "e", generation: 1n },
    attached: true,
    events: spec.events,
    endReason: spec.endReason ?? "deadlineReached",
    continuationGap: true,
    epoch,
  });
  const notified = spec.notifications ?? spec.events;
  return {
    request: {
      epoch: isBaseline
        ? null
        : spec.requestEpoch === undefined
          ? 7n
          : spec.requestEpoch === null
            ? null
            : BigInt(spec.requestEpoch),
      afterSequence: spec.after === null ? null : BigInt(spec.after),
    },
    result,
    notifications: notified.map((event) =>
      observationNotificationSchema.parse({
        event,
        cursor: "sequence" in event ? { epoch, afterSequence: event["sequence"] } : null,
      }),
    ),
  };
}

function promptResult(text: string, target: SessionRef = TARGET): unknown {
  return {
    kind: "completed",
    target,
    operationId: OPERATION,
    settlement: {
      target,
      stopReason: "endTurn",
      detail: { kind: "providerPrompt", output: { kind: "available", text } },
    },
  };
}
const SETTLED_OPERATION = {
  operationId: OPERATION,
  operation: "conversationPrompt",
  binding: {},
  target: TARGET,
  stage: "terminal",
  effect: "applied",
  reconciliation: "confirmed",
  // The snapshot's own snake_case vocabulary (ProviderPromptStopReason).
  terminalStopReason: "end_turn",
  admittedAt: "t",
};

function evidence(
  continuation: readonly ReadSpec[],
  overrides: Partial<PromptCaseEvidence> = {},
  baseline: readonly RawEvent[] = BASELINE,
): PromptCaseEvidence {
  const reads = [
    read({ after: null, events: baseline }, true),
    ...continuation.map((spec) => read(spec, false)),
  ];
  return {
    target: TARGET,
    operationId: OPERATION,
    expectedStopReason: "endTurn",
    ledger: foldObservationReads(TARGET, reads),
    reply: { kind: "promptResult", value: promptResult("beforeafter") },
    operation: SETTLED_OPERATION,
    otherOperations: 0,
    peerReceipt: { prompts: 1, cancels: 0, releases: 0, releasesFollowPrompt: true },
    expectedPeerCancels: 0,
    expectedPeerReleases: 0,
    ...overrides,
  };
}

test("one isolated prompt with matching reply, readback and one dispatch qualifies", () => {
  const verdict = decidePromptCase(evidence([{ after: 2, events: PROMPT_TURN }]));
  assert.deepEqual(verdict, {
    kind: "qualified",
    turnId: "turn-1",
    inputId: "input-1",
    reply: "beforeafter",
  });
});

test("an empty baseline and output split over two bounded reads still qualify", () => {
  const verdict = decidePromptCase(
    evidence(
      [
        { after: null, events: PROMPT_TURN.slice(0, 3), endReason: "resultLimitReached" },
        { after: 5, events: PROMPT_TURN.slice(3) },
      ],
      {},
      [],
    ),
  );
  assert.equal(verdict.kind, "qualified");
});

test("a result for the same raw ID on another endpoint is a wrong target", () => {
  const other: SessionRef = {
    ...TARGET,
    endpoint: { ...TARGET.endpoint, endpointId: "claude-local" },
  };
  const verdict = decidePromptCase(evidence([{ after: 2, events: PROMPT_TURN, target: other }]));
  assert.deepEqual(verdict, {
    kind: "failed",
    reason: "observation read 1: the result names another Session",
  });
  const reply = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      reply: { kind: "promptResult", value: promptResult("beforeafter", other) },
    }),
  );
  assert.equal(reply.kind, "failed");
});

test("a completed prompt with no observed turn end is a false completion", () => {
  const open = PROMPT_TURN.slice(0, 5);
  const verdict = decidePromptCase(evidence([{ after: 2, events: open }]));
  assert.equal(verdict.kind, "failed");
  const none = decidePromptCase(evidence([{ after: 2, events: [] }]));
  assert.deepEqual(none, {
    kind: "failed",
    reason: "no turn followed the baseline (false completion)",
  });
});

test("a reconstructed reply shorter than the operation's reply is lost or truncated output", () => {
  const truncated = PROMPT_TURN.filter((event) => event["sequence"] !== 6n);
  const verdict = decidePromptCase(evidence([{ after: 2, events: truncated }]));
  assert.deepEqual(verdict, {
    kind: "failed",
    reason: "the operation's reply differs from the reconstructed output (lost or truncated)",
  });
});

test("a second provider prompt for one input is duplicate dispatch", () => {
  const verdict = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      peerReceipt: { prompts: 2, cancels: 0, releases: 0, releasesFollowPrompt: true },
    }),
  );
  assert.deepEqual(verdict, {
    kind: "failed",
    reason: "the controlled peer received 2 prompts, not exactly one",
  });
});

test("a replayed or fabricated cursor is refused", () => {
  const replayed = decidePromptCase(
    evidence([
      { after: 2, events: [numbered(2, { kind: "stateChanged", state: {} }), ...PROMPT_TURN] },
    ]),
  );
  assert.equal(replayed.kind, "failed");
  const skipped = decidePromptCase(evidence([{ after: 5, events: PROMPT_TURN.slice(3) }]));
  assert.deepEqual(skipped, {
    kind: "failed",
    reason: "observation read 1: the request did not continue from the last observed cursor",
  });
});

test("a notification that disagrees with its result is refused", () => {
  const forged = [numbered(3, { kind: "turnStarted", turnId: "turn-9", inputId: "input-9" })];
  const verdict = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN, notifications: forged }]),
  );
  assert.deepEqual(verdict, {
    kind: "failed",
    reason: "observation read 1: notification 0 is not the result's event 0",
  });
});

test("a stale epoch's resync is inconclusive, never complete output", () => {
  const verdict = decidePromptCase(
    evidence([
      { after: 2, events: [{ kind: "resyncRequired" }], epoch: 8, endReason: "resyncRequired" },
    ]),
  );
  assert.deepEqual(verdict, { kind: "inconclusive", reason: "observation needs resync at read 1" });
});

test("an extra turn or operation on the isolated Session is inconclusive", () => {
  const second = promptTurn(9, "turn-2", "input-2");
  const extra = decidePromptCase(evidence([{ after: 2, events: [...PROMPT_TURN, ...second] }]));
  assert.deepEqual(extra, {
    kind: "inconclusive",
    reason: "more than one turn followed the baseline",
  });
  const shared = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], { otherOperations: 1 }),
  );
  assert.equal(shared.kind, "inconclusive");
});

test("a different stop reason in any one vocabulary is refused", () => {
  const cancelled = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      operation: { ...SETTLED_OPERATION, terminalStopReason: "cancelled" },
    }),
  );
  assert.deepEqual(cancelled, {
    kind: "failed",
    reason: "the operation's terminal stop reason differs from the observed turn",
  });
});

test("a detached caller's case is decided from the operation wait, including truncation", () => {
  const settlement = (response: string): unknown => ({
    kind: "available",
    settlement: { kind: "promptCompleted", target: TARGET, stopReason: "end_turn", response },
  });
  const whole = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      reply: { kind: "operationWait", output: settlement("beforeafter") },
    }),
  );
  assert.equal(whole.kind, "qualified");
  const short = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      reply: { kind: "operationWait", output: settlement("before") },
    }),
  );
  assert.equal(short.kind, "failed");
  const pending = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      reply: { kind: "operationWait", output: { kind: "pending" } },
    }),
  );
  assert.equal(pending.kind, "inconclusive");
});

test("an unsettled or foreign operation readback is refused", () => {
  const running = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      operation: { ...SETTLED_OPERATION, stage: "dispatched" },
    }),
  );
  assert.equal(running.kind, "failed");
  const foreign = decidePromptCase(
    evidence([{ after: 2, events: PROMPT_TURN }], {
      operation: { ...SETTLED_OPERATION, operationId: "op-other" },
    }),
  );
  assert.deepEqual(foreign, { kind: "failed", reason: "the readback names another operation" });
});

test("a cursor that skips past a second turn is refused, though the visible chain is otherwise valid", () => {
  // Turn 1 is read completely; the stream then held a second turn (9..14) that the next read
  // skips by naming afterSequence 14 instead of 8. Every visible event is valid and unduplicated,
  // so only the continuity check can see that a turn was hidden.
  const skipped = decidePromptCase(
    evidence([
      { after: 2, events: PROMPT_TURN, endReason: "resultLimitReached" },
      { after: 14, events: [] },
    ]),
  );
  assert.deepEqual(skipped, {
    kind: "failed",
    reason: "observation read 2: the request did not continue from the last observed cursor",
  });
});
