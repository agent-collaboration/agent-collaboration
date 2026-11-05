// The recorded MCP exchanges the cases are built from. Every call's arguments and raw outcome
// are appended to `exchanges.jsonl` before anything is decided from them, so a verdict can be
// rechecked from evidence. No step retries a mutation: an unconfirmed outcome is recorded and
// returned, and only operation show/wait reads learn what happened.
import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { encode } from "../../exact-json.ts";
import {
  boundedObservationResultSchema,
  OBSERVATION_NOTIFICATION_METHOD,
  observationNotificationSchema,
  type SessionRef,
} from "./observation-models.ts";
import { foldObservationReads, type ObservationRead } from "./observation-ledger.ts";
import { reconstructTurns } from "./observation-turns.ts";
import { observeArguments } from "./journey-requests.ts";
import { immediateAnswerProblem, settlementState } from "./operation-settlement.ts";
import { z } from "zod";
import type { McpCallOutcome, StreamingMcpClient } from "./streaming-mcp-client.ts";

export interface JourneyContext {
  readonly client: StreamingMcpClient;
  readonly evidence: string;
  readonly deadline: number;
}

export function openEvidence(evidence: string): void {
  mkdirSync(evidence, { recursive: true, mode: 0o700 });
}

export function record(context: JourneyContext, entry: Readonly<Record<string, unknown>>): void {
  appendFileSync(
    join(context.evidence, "exchanges.jsonl"),
    `${encode({ at: new Date().toISOString(), ...entry })}\n`,
  );
}

function remainingMs(context: JourneyContext): number {
  const remaining = context.deadline - Date.now();
  if (remaining <= 0) throw new Error("the run's deadline has passed; no further input");
  return remaining;
}

/** One recorded tools/call, bounded by both its own budget and the run's deadline. */
export async function callTool(
  context: JourneyContext,
  step: string,
  tool: string,
  arguments_: Readonly<Record<string, unknown>>,
  options: { readonly timeoutMs: number; readonly abort?: AbortSignal },
): Promise<McpCallOutcome> {
  const timeoutMs = Math.min(options.timeoutMs, remainingMs(context));
  const outcome = await context.client.callTool(tool, arguments_, {
    timeoutMs,
    ...(options.abort === undefined ? {} : { abort: options.abort }),
  });
  record(context, { step, tool, arguments: arguments_, outcome });
  return outcome;
}

/** The answered value, or an error naming the outcome; never a guessed success. */
export function answered(outcome: McpCallOutcome, what: string): unknown {
  if (outcome.kind === "answered") return outcome.value;
  throw new Error(
    `${what} was ${outcome.kind}${"reason" in outcome ? ` (${outcome.reason})` : ""}`,
  );
}

/** One bounded observation from `cursor`, as the ledger reads it. */
export async function observe(
  context: JourneyContext,
  step: string,
  target: SessionRef,
  cursor: { readonly epoch: bigint | null; readonly afterSequence: bigint | null },
  timeoutSeconds: number,
): Promise<ObservationRead> {
  const outcome = await callTool(
    context,
    step,
    "events_observe",
    observeArguments(target, cursor, timeoutSeconds),
    {
      timeoutMs: (timeoutSeconds + 10) * 1000,
    },
  );
  const result = boundedObservationResultSchema.parse(answered(outcome, `${step} observation`));
  const notifications = outcome.kind === "answered" ? outcome.notifications : [];
  return {
    request: cursor,
    result,
    notifications: notifications
      .filter((notification) => notification.method === OBSERVATION_NOTIFICATION_METHOD)
      .map((notification) => observationNotificationSchema.parse(notification.params)),
  };
}

/** The cursor the next read must send after `reads`; refuses a broken chain. */
export function nextCursor(
  target: SessionRef,
  reads: readonly ObservationRead[],
  baselineReads = 1,
): { readonly epoch: bigint; readonly afterSequence: bigint | null } {
  const ledger = foldObservationReads(target, reads, baselineReads);
  if (ledger.kind !== "continuous")
    throw new Error(`observation is ${ledger.kind}; it cannot be continued`);
  const last = ledger.events.at(-1)?.sequence ?? null;
  const tooLarge = ledger.tooLarge.at(-1)?.sequence ?? null;
  const highest =
    last === null ? tooLarge : tooLarge === null ? last : last > tooLarge ? last : tooLarge;
  return { epoch: ledger.epoch, afterSequence: highest ?? ledger.baselineSequence };
}

/** Continues from `reads` until the post-baseline history holds `turns` ended turns. */
export async function observeUntilTurnsEnd(
  context: JourneyContext,
  step: string,
  target: SessionRef,
  reads: ObservationRead[],
  turns: number,
  baselineReads = 1,
): Promise<void> {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    const ledger = foldObservationReads(target, reads, baselineReads);
    if (ledger.kind !== "continuous") return;
    const reconstruction = reconstructTurns(ledger.events);
    if (reconstruction.kind === "rejected") return;
    const ended = reconstruction.turns.filter((turn) => turn.outcome !== null).length;
    if (ended >= turns || ledger.backendDisconnected) return;
    reads.push(
      await observe(
        context,
        `${step}-${attempt}`,
        target,
        nextCursor(target, reads, baselineReads),
        2,
      ),
    );
  }
}

/** Bounded reads per reattach baseline: up to 200 events each, so 2000 replayed events in all. */
export const REPLAY_BASELINE_READS = 10;

/**
 * A baseline that continues while the Host reports `resultLimitReached`, so a long replayed
 * history is read whole before the next input. Returns the reads, or null when the declared
 * bound ran out first; a truncated baseline must never be decided as complete history.
 */
export async function observeReplayBaseline(
  context: JourneyContext,
  step: string,
  target: SessionRef,
): Promise<ObservationRead[] | null> {
  const reads = [
    await observe(context, `${step}-0`, target, { epoch: null, afterSequence: null }, 1),
  ];
  for (let read = 1; read < REPLAY_BASELINE_READS; read += 1) {
    if (reads.at(-1)?.result.endReason !== "resultLimitReached") return reads;
    reads.push(
      await observe(context, `${step}-${read}`, target, nextCursor(target, reads, reads.length), 1),
    );
  }
  return reads.at(-1)?.result.endReason === "resultLimitReached" ? null : reads;
}

/** Reads the operation's snapshot without changing it. */
export async function operationShow(
  context: JourneyContext,
  step: string,
  operationId: string,
): Promise<unknown> {
  const outcome = await callTool(
    context,
    step,
    "conversation_operation_show",
    { operationId },
    { timeoutMs: 15_000 },
  );
  return answered(outcome, `${step} operation show`);
}

/** Waits for the operation to settle; a bounded read, never a resubmission. */
export async function operationWait(
  context: JourneyContext,
  step: string,
  operationId: string,
  timeoutSeconds: number,
): Promise<unknown> {
  const outcome = await callTool(
    context,
    step,
    "conversation_operation_wait",
    { operationId, timeoutSeconds },
    { timeoutMs: (timeoutSeconds + 10) * 1000 },
  );
  return answered(outcome, `${step} operation wait`);
}

/**
 * Follows the operation the caller supplied until it is terminal, applied and confirmed on the
 * exact target (operation-settlement.ts). One bounded wait if it is still open; no replay.
 */
export async function requireSettled(
  context: JourneyContext,
  step: string,
  answer: unknown,
  operationId: string,
  target: SessionRef,
): Promise<void> {
  const problem = immediateAnswerProblem(answer, operationId, target);
  if (problem !== null) throw new Error(`${step}: ${problem}`);
  let state = settlementState(
    await operationShow(context, `${step}-show`, operationId),
    operationId,
    target,
  );
  if (state.kind === "open") {
    const waited = z
      .looseObject({ operation: z.unknown() })
      .parse(await operationWait(context, `${step}-wait`, operationId, 60));
    state = settlementState(waited.operation, operationId, target);
  }
  if (state.kind === "open")
    throw new Error(`${step}: still ${state.stage} at its wait bound; not replayed`);
  if (state.kind === "refused") throw new Error(`${step}: ${state.reason}`);
}

/** The binding generation the latest observation reported, which a cancel must name. */
export function observedGeneration(reads: readonly ObservationRead[]): unknown {
  const latest = reads.at(-1);
  if (latest === undefined) throw new Error("no observation has reported the binding generation");
  return latest.result.generation;
}
