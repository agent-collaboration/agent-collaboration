// Folds a baseline and the bounded `events_observe` reads that continue from it into one
// ordered event history for one exact Session, refusing anything the observation contract
// rules out. It decides cursor continuity and notification agreement only; what the events
// mean for a turn is observation-turns.ts.
//
// Contract (collaboration-service provider_observation_operations.rs:76-160 and
// collaboration-protocol session_observation_contract.rs:66-101): a call replays retained
// events with `sequence > afterSequence`, then live ones; a stale epoch returns only an
// unnumbered resync marker; every result has `continuationGap: true`, which alone is not loss;
// each streamed notification is the result's event "exactly as the call's result lists it",
// with cursor `{epoch, afterSequence: <its own sequence>}`, or null for a resync marker.
import { encode } from "../../../exact-json.ts";
import {
  eventSequence,
  sameSession,
  type BoundedObservationResult,
  type ObservationCursor,
  type ObservationNotification,
  type ObservedEvent,
  type SessionRef,
} from "./observation-models.ts";

/** One `events_observe` call: the cursor it sent, its result and its streamed notifications. */
export interface ObservationRead {
  readonly request: { readonly epoch: bigint | null; readonly afterSequence: bigint | null };
  readonly result: BoundedObservationResult;
  readonly notifications: readonly ObservationNotification[];
}

export interface NumberedEvent {
  readonly sequence: bigint;
  readonly event: Extract<ObservedEvent, { readonly event: unknown }>["event"];
}

export type ObservationLedger =
  | {
      readonly kind: "continuous";
      readonly epoch: bigint;
      /** The baseline's last sequence; null when the baseline held no numbered event. */
      readonly baselineSequence: bigint | null;
      /** The baseline's own numbered events, such as history a reattach replayed. */
      readonly baselineEvents: readonly NumberedEvent[];
      /** Oversized-event markers inside the baseline; replayed history they hide is not complete. */
      readonly baselineTooLarge: readonly {
        readonly sequence: bigint;
        readonly itemId: string | null;
      }[];
      readonly events: readonly NumberedEvent[];
      readonly tooLarge: readonly { readonly sequence: bigint; readonly itemId: string | null }[];
      readonly backendDisconnected: boolean;
    }
  | { readonly kind: "resyncRequired"; readonly read: number }
  | { readonly kind: "rejected"; readonly read: number; readonly reason: string };

interface Fold {
  epoch: bigint | null;
  last: bigint | null;
  baselineSequence: bigint | null;
  baselineEvents: NumberedEvent[];
  baselineTooLarge: { sequence: bigint; itemId: string | null }[];
  events: NumberedEvent[];
  tooLarge: { sequence: bigint; itemId: string | null }[];
  disconnected: boolean;
}

/**
 * Folds the baseline and every later continuation. The baseline is `reads[0]`, sent with no
 * cursor, plus the next `baselineReads - 1` reads that continued it; a result-limited baseline
 * (a long replayed history) spans several reads, and every read still chains by cursor.
 */
export function foldObservationReads(
  target: SessionRef,
  reads: readonly ObservationRead[],
  baselineReads = 1,
): ObservationLedger {
  if (reads.length === 0) return { kind: "rejected", read: 0, reason: "no baseline read" };
  const fold: Fold = {
    epoch: null,
    last: null,
    baselineSequence: null,
    baselineEvents: [],
    baselineTooLarge: [],
    events: [],
    tooLarge: [],
    disconnected: false,
  };
  for (const [index, read] of reads.entries()) {
    const outcome = foldRead(target, fold, read, index);
    if (outcome !== null) return outcome;
    if (index === baselineReads - 1) {
      fold.baselineSequence = fold.last;
      fold.baselineEvents = fold.events;
      fold.baselineTooLarge = fold.tooLarge;
      fold.events = [];
      fold.tooLarge = [];
    }
  }
  if (fold.epoch === null)
    return { kind: "rejected", read: 0, reason: "a provider observation reported no epoch" };
  return {
    kind: "continuous",
    epoch: fold.epoch,
    baselineSequence: fold.baselineSequence,
    baselineEvents: fold.baselineEvents,
    baselineTooLarge: fold.baselineTooLarge,
    events: fold.events,
    tooLarge: fold.tooLarge,
    backendDisconnected: fold.disconnected,
  };
}

function rejected(read: number, reason: string): ObservationLedger {
  return { kind: "rejected", read, reason };
}

function foldRead(
  target: SessionRef,
  fold: Fold,
  read: ObservationRead,
  index: number,
): ObservationLedger | null {
  const { request, result } = read;
  if (!sameSession(result.target, target))
    return rejected(index, "the result names another Session");
  if (fold.disconnected) return rejected(index, "a read continued after the backend disconnected");
  const expectedCursor =
    index === 0
      ? { epoch: null, afterSequence: null }
      : { epoch: fold.epoch, afterSequence: fold.last };
  if (
    request.epoch !== expectedCursor.epoch ||
    request.afterSequence !== expectedCursor.afterSequence
  )
    return rejected(index, "the request did not continue from the last observed cursor");
  if (result.epoch === null || result.epoch === undefined)
    return rejected(index, "a provider observation reported no epoch");
  if (fold.epoch !== null && result.epoch !== fold.epoch) {
    return result.endReason === "resyncRequired" && isOnlyBareResync(result)
      ? { kind: "resyncRequired", read: index }
      : rejected(index, "the epoch changed without a resync marker");
  }
  fold.epoch = result.epoch;
  const notified = checkNotifications(read, result.epoch);
  if (notified !== null) return rejected(index, notified);
  for (const [position, event] of result.events.entries()) {
    const outcome = foldEvent(fold, event, position === result.events.length - 1, result);
    if (outcome === "resync") return { kind: "resyncRequired", read: index };
    if (outcome !== null) return rejected(index, outcome);
  }
  if (result.endReason === "resyncRequired")
    return rejected(index, "resyncRequired ended the call without a resync marker");
  fold.disconnected = result.endReason === "backendDisconnected";
  return null;
}

function isOnlyBareResync(result: BoundedObservationResult): boolean {
  const only = result.events.length === 1 ? result.events[0] : undefined;
  return only !== undefined && eventSequence(only) === null;
}

/** null when the event is accepted; "resync" for a final marker; otherwise why it is refused. */
function foldEvent(
  fold: Fold,
  event: ObservedEvent,
  isLast: boolean,
  result: BoundedObservationResult,
): string | "resync" | null {
  if (!("sequence" in event))
    return isLast && result.endReason === "resyncRequired"
      ? "resync"
      : "an unnumbered resync marker was not the call's last event";
  const { sequence } = event;
  if (fold.last !== null && sequence <= fold.last)
    return "a sequence did not advance past the cursor (replayed or fabricated)";
  fold.last = sequence;
  if (!("event" in event)) {
    fold.tooLarge.push({ sequence, itemId: event.itemId ?? null });
    return null;
  }
  if (event.event.kind === "resyncRequired")
    return isLast && result.endReason === "resyncRequired"
      ? "resync"
      : "a hub resync event was not the call's last event";
  fold.events.push({ sequence, event: event.event });
  return null;
}

/**
 * Notifications are the result's events in order; the server may leave a tail unsent once the
 * call's bound passes (carrier_tools.rs stream_observed_events), never send an extra one.
 */
function checkNotifications(read: ObservationRead, epoch: bigint): string | null {
  const { notifications, result } = read;
  if (notifications.length > result.events.length) return "more notifications than result events";
  for (const [index, notification] of notifications.entries()) {
    const event = result.events[index];
    if (event === undefined || encode(notification.event) !== encode(event))
      return `notification ${index} is not the result's event ${index}`;
    const sequence = eventSequence(event);
    const expected: ObservationCursor | null =
      sequence === null || ("event" in event && event.event.kind === "resyncRequired")
        ? null
        : { epoch, afterSequence: sequence };
    if (encode(notification.cursor ?? null) !== encode(expected))
      return `notification ${index} carries the wrong cursor`;
  }
  return null;
}
