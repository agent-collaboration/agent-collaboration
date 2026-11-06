// Wire shapes the MCP control journey reads, parsed only for the fields it decides on. Integers
// arrive exact (exact-json `decode` yields bigint), so sequences and epochs are bigint here.
// Sources: collaboration-protocol session_observation_contract.rs (bounded result, cursor and
// notification), collaboration-service provider_observation_operations.rs:269 (the
// `{sequence, event}` envelope) and session-event-model session_event.rs:120-242 (events).
import { z } from "zod";

export const sessionRefSchema = z.object({
  endpoint: z.object({ serviceId: z.string().min(1), endpointId: z.string().min(1) }),
  sessionId: z.string().min(1),
});
export type SessionRef = z.infer<typeof sessionRefSchema>;

export function sameSession(left: SessionRef, right: SessionRef): boolean {
  return (
    left.sessionId === right.sessionId &&
    left.endpoint.serviceId === right.endpoint.serviceId &&
    left.endpoint.endpointId === right.endpoint.endpointId
  );
}

const sessionItemSchema = z.looseObject({
  itemId: z.string().min(1),
  kind: z.looseObject({ kind: z.string().min(1) }),
  text: z.string().nullable().optional(),
});
export type SessionItem = z.infer<typeof sessionItemSchema>;

// TurnOutcome (session_event.rs:141-151) renames only its variant tag, so the ended variant's
// fields stay snake_case on the wire, unlike the camelCase event fields around them; it denies
// unknown fields, so an invented camelCase form is refused here too. StopReason and LocalCause
// are externally tagged: a known value is a camelCase string, `Unknown(text)` is
// `{unknown: text}` and `Other(text)` is `{other: text}` (session_event.rs:5-29).
const stopReasonSchema = z.union([
  z.enum(["endTurn", "maxTokens", "maxTurnRequests", "refusal", "cancelled"]),
  z.strictObject({ unknown: z.string() }),
]);
const localCauseSchema = z.union([
  z.literal("outputOverflow"),
  z.strictObject({ other: z.string() }),
]);
export const turnOutcomeSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("ended"),
    stop_reason: stopReasonSchema,
    local_cause: localCauseSchema.nullable(),
  }),
  z.strictObject({
    kind: z.literal("lost"),
    reason: z.enum(["providerRetired", "providerTurnFailed", "endNotObservable"]),
  }),
]);
export type TurnOutcome = z.infer<typeof turnOutcomeSchema>;

// Every SessionEvent kind is named so an unknown kind fails loudly instead of being skipped.
// A direct prompt's acceptance is its turnStarted (its own input ID at dispatch); inputAccepted
// is published only for a steer injected into an already active turn
// (acp-client-runtime provider_session_actor.rs:398-407).
export const sessionEventSchema = z.discriminatedUnion("kind", [
  z.looseObject({
    kind: z.literal("turnStarted"),
    turnId: z.string().min(1),
    inputId: z.string().min(1),
  }),
  z.looseObject({
    kind: z.literal("inputAccepted"),
    inputId: z.string().min(1),
    turnId: z.string().min(1),
  }),
  z.looseObject({ kind: z.literal("turnEnded"), turnId: z.string(), outcome: turnOutcomeSchema }),
  z.looseObject({ kind: z.literal("itemStarted"), item: sessionItemSchema }),
  z.looseObject({ kind: z.literal("itemUpdated"), item: sessionItemSchema }),
  z.looseObject({ kind: z.literal("itemCompleted"), itemId: z.string() }),
  z.looseObject({ kind: z.literal("interactionRequested"), interaction: z.unknown() }),
  z.looseObject({ kind: z.literal("interactionResolved"), requestId: z.string() }),
  z.looseObject({ kind: z.literal("stateChanged"), state: z.unknown() }),
  z.looseObject({ kind: z.literal("capabilitiesChanged"), capabilities: z.unknown() }),
  z.looseObject({ kind: z.literal("settingsChanged"), settings: z.unknown() }),
  z.looseObject({ kind: z.literal("resyncRequired"), replayEpoch: z.bigint() }),
]);
export type SessionEvent = z.infer<typeof sessionEventSchema>;

// An oversized event is replaced in place by this marker (event_value_bounded,
// provider_observation_operations.rs:267-292).
const eventTooLargeSchema = z.strictObject({
  kind: z.literal("eventTooLarge"),
  sequence: z.bigint(),
  itemId: z.string().nullable().optional(),
});
// A stale epoch or a lagging reader yields this unnumbered marker as the call's last event
// (provider_observation_operations.rs:89-101,133-136); a hub reset yields a numbered
// `resyncRequired` event instead. Either way the history must be attached again.
const bareResyncMarkerSchema = z.strictObject({ kind: z.literal("resyncRequired") });

export const observedEventSchema = z.union([
  z.strictObject({ sequence: z.bigint(), event: sessionEventSchema }),
  eventTooLargeSchema,
  bareResyncMarkerSchema,
]);
export type ObservedEvent = z.infer<typeof observedEventSchema>;

export const observationEndReasonSchema = z.enum([
  "deadlineReached",
  "resultLimitReached",
  "callerCancelled",
  "backendDisconnected",
  "resyncRequired",
]);

export const boundedObservationResultSchema = z.strictObject({
  target: sessionRefSchema,
  generation: z.unknown(),
  attached: z.boolean(),
  events: z.array(observedEventSchema),
  endReason: observationEndReasonSchema,
  continuationGap: z.boolean(),
  epoch: z.bigint().nullable().optional(),
});
export type BoundedObservationResult = z.infer<typeof boundedObservationResultSchema>;

export const observationCursorSchema = z.strictObject({
  epoch: z.bigint(),
  afterSequence: z.bigint(),
});
export type ObservationCursor = z.infer<typeof observationCursorSchema>;

export const OBSERVATION_NOTIFICATION_METHOD = "notifications/codexRouter/observationEvent";
export const observationNotificationSchema = z.strictObject({
  event: observedEventSchema,
  cursor: observationCursorSchema.nullable().optional(),
});
export type ObservationNotification = z.infer<typeof observationNotificationSchema>;

/** The hub position of an observed event; an unnumbered resync marker has none. */
export function eventSequence(event: ObservedEvent): bigint | null {
  return "sequence" in event ? event.sequence : null;
}
