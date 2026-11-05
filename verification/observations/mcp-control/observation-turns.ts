// Reconstructs the turns an observation ledger saw after its baseline. Item text is the item's
// latest retained state: the runtime appends each text chunk to the item and re-emits the whole
// item (acp-client-runtime provider_item_projection.rs:293-341), and a retained snapshot keeps
// latest state rather than every delta. So a missed intermediate update is not lost output; a
// final text that disagrees with the operation's own reply is.
import type { SessionItem } from "./observation-models.ts";
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
  readonly accepted: boolean;
  /** `{kind: "ended", stopReason}` or `{kind: "lost", reason}` as the hub reported it. */
  readonly outcome: unknown;
  readonly items: readonly ObservedItem[];
}

export type TurnReconstruction =
  | { readonly kind: "reconstructed"; readonly turns: readonly ObservedTurn[] }
  | { readonly kind: "rejected"; readonly sequence: bigint; readonly reason: string };

interface MutableTurn {
  turnId: string;
  inputId: string;
  accepted: boolean;
  outcome: unknown;
  items: Map<string, { kind: string; text: string | null; completed: boolean }>;
}

export function reconstructTurns(events: readonly NumberedEvent[]): TurnReconstruction {
  const turns: MutableTurn[] = [];
  let active: MutableTurn | null = null;
  for (const { sequence, event } of events) {
    const reason = applyEvent(turns, active, event);
    if (typeof reason === "string") return { kind: "rejected", sequence, reason };
    active = reason.active;
  }
  return {
    kind: "reconstructed",
    turns: turns.map((turn): ObservedTurn => ({
      turnId: turn.turnId,
      inputId: turn.inputId,
      accepted: turn.accepted,
      outcome: turn.outcome,
      items: [...turn.items].map(([itemId, item]): ObservedItem => ({ itemId, ...item })),
    })),
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
        accepted: false,
        outcome: null,
        items: new Map(),
      };
      turns.push(turn);
      return { active: turn };
    }
    case "inputAccepted": {
      const turn = turns.find((candidate): boolean => candidate.turnId === event.turnId);
      if (turn !== undefined) turn.accepted = turn.inputId === event.inputId || turn.accepted;
      return { active };
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
