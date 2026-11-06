// Decides the history a reattach exposed in its new epoch's baseline. Load replays the recorded
// conversation as historical turns: each opens with turnStarted (a fresh turn ID and generated
// input ID), carries the replayed user and agent items and ends `stop_reason: {unknown:
// "replayed"}` (acp-client-runtime provider_session_restore.rs:116-216). Resume replays nothing
// (provider_session_event_hub.rs begin_history_unavailable). A settled snapshot cannot decide
// either; only the events can. The recorded exchanges are the driver's own earlier observations.
import type { NumberedEvent } from "./observation-ledger.ts";
import { agentReplyText, reconstructTurns, type ObservedTurn } from "./observation-turns.ts";

export interface RecordedExchange {
  readonly prompt: string;
  readonly reply: string;
}

export type HistoryDecision =
  | { readonly kind: "complete" }
  | { readonly kind: "failed"; readonly reason: string };

const failed = (reason: string): HistoryDecision => ({ kind: "failed", reason });

/** The historical turns in `baseline`, or why they cannot be read as history. */
function historicalTurns(
  baseline: readonly NumberedEvent[],
): { readonly kind: "turns"; readonly turns: readonly ObservedTurn[] } | HistoryDecision {
  const reconstruction = reconstructTurns(baseline);
  if (reconstruction.kind === "rejected")
    return failed(`replayed history event ${reconstruction.sequence}: ${reconstruction.reason}`);
  if (reconstruction.acceptedInputs.length > 0)
    return failed("replayed history carries an accepted input, which a replay never publishes");
  return { kind: "turns", turns: reconstruction.turns };
}

function isReplayedTerminal(turn: ObservedTurn): boolean {
  const outcome = turn.outcome;
  return (
    outcome !== null &&
    outcome.kind === "ended" &&
    outcome.local_cause === null &&
    typeof outcome.stop_reason === "object" &&
    outcome.stop_reason.unknown === "replayed"
  );
}

function userText(turn: ObservedTurn): string {
  return turn.items
    .filter((item): boolean => item.kind === "userMessage")
    .map((item): string => item.text ?? "")
    .join("");
}

/** Load: exactly the recorded exchanges, in order, each a historical turn ending replayed. */
export function decideReplayedHistory(
  baseline: readonly NumberedEvent[],
  recorded: readonly RecordedExchange[],
): HistoryDecision {
  const read = historicalTurns(baseline);
  if (read.kind !== "turns") return read;
  const { turns } = read;
  if (turns.length !== recorded.length)
    return failed(`the replay holds ${turns.length} turns, not the ${recorded.length} recorded`);
  for (const [index, turn] of turns.entries()) {
    const exchange = recorded[index];
    if (exchange === undefined) return failed("the replay holds an unrecorded turn");
    if (!isReplayedTerminal(turn))
      return failed(`replayed turn ${index} does not end as replayed history`);
    if (userText(turn) !== exchange.prompt)
      return failed(`replayed turn ${index} is not the recorded input`);
    if (agentReplyText(turn) !== exchange.reply)
      return failed(`replayed turn ${index} is not the recorded reply`);
  }
  return { kind: "complete" };
}

/** Resume: no historical turn at all; the Session continues without replay. */
export function decideNoReplay(baseline: readonly NumberedEvent[]): HistoryDecision {
  const read = historicalTurns(baseline);
  if (read.kind !== "turns") return read;
  return read.turns.length === 0
    ? { kind: "complete" }
    : failed(`resume exposed ${read.turns.length} historical turns; it replays none`);
}
