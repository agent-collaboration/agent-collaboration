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

const turnOutcomeSchema = z.discriminatedUnion("kind", [
  z.looseObject({ kind: z.literal("ended"), stopReason: z.string().min(1) }),
  z.looseObject({ kind: z.literal("lost"), reason: z.unknown() }),
]);

// Every SessionEvent kind is named so an unknown kind fails loudly instead of being skipped.
export const sessionEventSchema = z.discriminatedUnion("kind", [
  z.looseObject({ kind: z.literal("turnStarted"), turnId: z.string(), inputId: z.string() }),
  z.looseObject({ kind: z.literal("inputAccepted"), inputId: z.string(), turnId: z.string() }),
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
