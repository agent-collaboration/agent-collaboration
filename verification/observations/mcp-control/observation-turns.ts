// Reconstructs the turns an observation ledger saw after its baseline. Item text is the item's
// latest retained state: the runtime appends each text chunk to the item and re-emits the whole
// item (acp-client-runtime provider_item_projection.rs:293-341), and a retained snapshot keeps
// latest state rather than every delta. So a missed intermediate update is not lost output; a
// final text that disagrees with the operation's own reply is.
//
// A direct prompt is accepted at its turnStarted, which carries its own input ID. inputAccepted is
// published only when a steer is injected into an already active turn
// (provider_session_actor.rs:398-407), so every one seen is another input; it is kept, never
// folded into the turn it names, and the isolation rule decides what it means.
import type { SessionItem, TurnOutcome } from "./observation-models.ts";
import type { NumberedEvent } from "./observation-ledger.ts";

export interface ObservedItem {
  readonly itemId: string;
  readonly kind: string;
  readonly text: string | null;
  readonly completed: boolean;
}

export interface ObservedTurn {
  readonly turnId: string;
  readonly inputId: string;
  /** The turn's end as the hub reported it; null while the turn is still open. */
  readonly outcome: TurnOutcome | null;
  readonly items: readonly ObservedItem[];
}

/** An injected input the Session accepted, named by the turn it joined. */
export interface AcceptedInput {
  readonly turnId: string;
  readonly inputId: string;
}

export type TurnReconstruction =
  | {
      readonly kind: "reconstructed";
      readonly turns: readonly ObservedTurn[];
      readonly acceptedInputs: readonly AcceptedInput[];
    }
  | { readonly kind: "rejected"; readonly sequence: bigint; readonly reason: string };

interface MutableTurn {
  turnId: string;
  inputId: string;
  outcome: TurnOutcome | null;
  items: Map<string, { kind: string; text: string | null; completed: boolean }>;
}

export function reconstructTurns(events: readonly NumberedEvent[]): TurnReconstruction {
  const turns: MutableTurn[] = [];
  const acceptedInputs: AcceptedInput[] = [];
  let active: MutableTurn | null = null;
  for (const { sequence, event } of events) {
    if (event.kind === "inputAccepted") {
      acceptedInputs.push({ turnId: event.turnId, inputId: event.inputId });
      continue;
    }
    const reason = applyEvent(turns, active, event);
    if (typeof reason === "string") return { kind: "rejected", sequence, reason };
    active = reason.active;
  }
  return {
    kind: "reconstructed",
    turns: turns.map((turn): ObservedTurn => ({
      turnId: turn.turnId,
      inputId: turn.inputId,
      outcome: turn.outcome,
      items: [...turn.items].map(([itemId, item]): ObservedItem => ({ itemId, ...item })),
    })),
    acceptedInputs,
  };
}

/** The agent's reply in one turn: its agent-message items' final texts, in start order. */
export function agentReplyText(turn: ObservedTurn): string {
  return turn.items
    .filter((item): boolean => item.kind === "agentMessage")
    .map((item): string => item.text ?? "")
    .join("");
}

function applyEvent(
  turns: MutableTurn[],
  active: MutableTurn | null,
  event: NumberedEvent["event"],
): { readonly active: MutableTurn | null } | string {
  switch (event.kind) {
    case "turnStarted": {
      if (active !== null) return "a turn started before the previous one ended";
      const turn: MutableTurn = {
        turnId: event.turnId,
        inputId: event.inputId,
        outcome: null,
        items: new Map(),
      };
      turns.push(turn);
      return { active: turn };
    }
    case "turnEnded": {
      if (active === null || active.turnId !== event.turnId)
        return "a turn ended that was not the active turn";
      active.outcome = event.outcome;
      return { active: null };
    }
    case "itemStarted":
    case "itemUpdated":
      return applyItem(active, event.item);
    case "itemCompleted": {
      const item = active?.items.get(event.itemId);
      if (item !== undefined) item.completed = true;
      return { active };
    }
    default:
      return { active };
  }
}

function applyItem(
  active: MutableTurn | null,
  item: SessionItem,
): { readonly active: MutableTurn | null } {
  // Items outside a turn (startup notices, settings) belong to no input and are not output.
  if (active === null) return { active };
  const previous = active.items.get(item.itemId);
  active.items.set(item.itemId, {
    kind: item.kind.kind,
    text: item.text ?? previous?.text ?? null,
    completed: previous?.completed ?? false,
  });
  return { active };
}
