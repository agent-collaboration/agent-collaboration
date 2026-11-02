import { z } from "zod";
import { objectSchema } from "../exact-json.ts";
import { decodeEvidenceJson } from "../evidence-json.ts";

export const exactIntegerSchema = z.bigint();
export const jsonObjectSchema = objectSchema;
export type JsonObject = z.infer<typeof jsonObjectSchema>;

export function observedObject(value: unknown): JsonObject {
  return jsonObjectSchema.parse(value);
}
export function readObservedJson(value: string | Buffer): JsonObject {
  return observedObject(decodeEvidenceJson(value));
}

export const endpointViewSchema = z.looseObject({ serviceId: z.string(), endpointId: z.string() });
export const sessionViewSchema = z.looseObject({
  endpoint: endpointViewSchema,
  sessionId: z.string(),
});
export const generationViewSchema = z.looseObject({
  serviceEpoch: z.string(),
  generation: exactIntegerSchema,
});
export const bindingViewSchema = z.looseObject({
  binding: z.looseObject({ generation: generationViewSchema }),
});
export const operationViewSchema = z.looseObject({
  target: sessionViewSchema,
  binding: bindingViewSchema,
  inputId: z.string().nullable().default(null),
  stage: z.string().nullable().default(null),
  effect: z.string().nullable().default(null),
  reconciliation: z.string().nullable().default(null),
});
export const hostManifestSchema = z.looseObject({
  serviceId: z.string(),
  serviceEpoch: z.string(),
  mcp: z.looseObject({ url: z.string() }),
  serviceVersion: z.string(),
  machineLabel: z.string(),
  routerProxyEndpoint: z.string(),
});
export const routerStartedSchema = z.looseObject({ binarySHA256: z.string(), head: z.string() });
export const harnessClaimsSchema = z.looseObject({
  rows: z.array(z.tuple([z.string(), z.string(), z.string()])),
  details: z.record(z.string(), z.string()),
  scope: z.string(),
});
// Ownership follows the launching account's real UID (ps `ruid`, Node `process.getuid()`); the
// effective UID (ps `uid`) is evidence only, because a user-launched setuid utility changes it.
export const processIdentitySchema = z.object({
  pid: z.coerce.number().int(),
  parent: z.coerce.number().int(),
  realUid: z.coerce.number().int(),
  effectiveUid: z.coerce.number().int(),
  group: z.coerce.number().int(),
  birth: z.string(),
  executable: z.string(),
});
export type ProcessIdentity = z.infer<typeof processIdentitySchema>;
export const identityRecordSchema = z.record(z.string(), z.union([z.number().int(), z.string()]));
export type IdentityRecord = z.infer<typeof identityRecordSchema>;
export const qualifiedBinarySchema = z.looseObject({
  target: z.string(),
  path: z.string(),
  sha256: z.string(),
  testGuardFeatures: z.array(z.string()),
});
export type QualifiedBinary = z.infer<typeof qualifiedBinarySchema>;
export const binaryQualificationSchema = z.looseObject({
  binaries: z.array(qualifiedBinarySchema),
});
export const authStatusSchema = z.looseObject({ kind: z.string() });
export const providerCapabilitiesSchema = z.looseObject({
  elicitation: z.boolean(),
  authStatus: authStatusSchema.nullable().default(null),
});
export const endpointInventorySchema = z.looseObject({
  endpoints: z.array(
    z.looseObject({
      endpoint: endpointViewSchema,
      availability: z.looseObject({ state: z.string() }),
      channels: z.array(
        z.looseObject({
          kind: z.string(),
          bindingGeneration: exactIntegerSchema.nullable().default(null),
        }),
      ),
    }),
  ),
});
export const toolContentSchema = z.looseObject({
  type: z.string(),
  text: z.string().nullable().default(null),
});
export const toolResultSchema = z.looseObject({
  isError: z.boolean().default(false),
  structuredContent: jsonObjectSchema.nullable().default(null),
  content: z.array(toolContentSchema).default([]),
});
export const rpcEnvelopeSchema = z.looseObject({
  id: exactIntegerSchema,
  result: toolResultSchema.nullable().default(null),
  error: z.looseObject({ code: exactIntegerSchema }).nullable().default(null),
});
export const publicOptionSchema = z.looseObject({
  effect: z.string(),
  scope: z.string(),
  optionId: z.string(),
  persistentTarget: z.string().nullable().default(null),
});
export type PublicOption = z.infer<typeof publicOptionSchema>;
export const publicInteractionSchema = z.looseObject({
  requester: sessionViewSchema,
  approver: jsonObjectSchema,
  requestId: z.string(),
  options: z.array(publicOptionSchema).default([]),
});
export const interactionListingSchema = z.looseObject({
  questions: z.array(publicInteractionSchema).default([]),
  approvals: z.array(publicInteractionSchema).default([]),
});
export const publicObservationSchema = z.looseObject({
  events: z.array(z.looseObject({ sequence: exactIntegerSchema, event: jsonObjectSchema })),
});
export const settingsChoiceSchema = z.looseObject({ value: z.string() });
export const settingsValueSchema = z.looseObject({
  kind: z.string(),
  value: z.union([z.string(), z.boolean()]),
});
export const settingsOptionSchema = z.looseObject({
  id: z.string(),
  category: z.string().nullable().default(null),
  currentValue: settingsValueSchema,
  choices: z.array(settingsChoiceSchema),
});
export const settingsCatalogSchema = z.looseObject({
  currentMode: z.string(),
  modes: z.array(settingsChoiceSchema),
  configOptions: z.array(settingsOptionSchema),
});
export const catalogRecordSchema = z.looseObject({ settingsCatalog: settingsCatalogSchema });
export const providerRecordSchema = z.looseObject({
  target: sessionViewSchema,
  state: z.string(),
  capabilities: providerCapabilitiesSchema,
  settingsCatalog: settingsCatalogSchema,
});
export const questionViewSchema = z.looseObject({
  fields: z.array(
    z.looseObject({
      kind: z.string(),
      fieldId: z.string(),
      required: z.boolean(),
      options: z.array(z.looseObject({ optionId: z.string(), label: z.string() })).default([]),
    }),
  ),
});
export type QuestionView = z.infer<typeof questionViewSchema>;
export const interactionRequestSchema = z.looseObject({
  requestId: z.string(),
  subject: z
    .looseObject({
      toolCall: z.looseObject({
        toolCallId: z.string(),
        kind: z.string().nullable().default(null),
      }),
    })
    .nullable()
    .default(null),
  description: z.string().nullable().default(null),
});
export const requestedInteractionSchema = z.looseObject({
  kind: z.string(),
  request: interactionRequestSchema,
});

export const turnStartedEventSchema = z.looseObject({
  kind: z.literal("turnStarted"),
  turnId: z.string(),
  inputId: z.string(),
});
export const turnEndedEventSchema = z.looseObject({
  kind: z.literal("turnEnded"),
  turnId: z.string(),
  outcome: z.looseObject({ kind: z.string(), stop_reason: z.string().nullable().default(null) }),
});
export const inputAcceptedEventSchema = z.looseObject({
  kind: z.literal("inputAccepted"),
  turnId: z.string(),
  inputId: z.string(),
});
export const interactionRequestedEventSchema = z.looseObject({
  kind: z.literal("interactionRequested"),
  interaction: requestedInteractionSchema,
});
export const interactionResolvedEventSchema = z.looseObject({
  kind: z.literal("interactionResolved"),
  requestId: z.string(),
});
export const toolItemEventSchema = z.looseObject({
  kind: z.enum(["itemStarted", "itemUpdated"]),
  item: z.looseObject({
    itemId: z.string(),
    kind: z.looseObject({ kind: z.literal("toolCall"), status: z.string() }),
  }),
});
export const callbackEventSchema = z.discriminatedUnion("kind", [
  turnStartedEventSchema,
  turnEndedEventSchema,
  inputAcceptedEventSchema,
  interactionRequestedEventSchema,
  interactionResolvedEventSchema,
  toolItemEventSchema,
]);
export const sequencedEventSchema = z.looseObject({
  sequence: exactIntegerSchema,
  event: callbackEventSchema,
});
export const observationViewSchema = z.looseObject({
  target: sessionViewSchema,
  generation: generationViewSchema,
  attached: z.boolean(),
  endReason: z.string(),
});
export const callbackCaptureSchema = z.looseObject({
  target: sessionViewSchema,
  observation: observationViewSchema,
  events: z.array(sequencedEventSchema),
});
export type CallbackCapture = z.infer<typeof callbackCaptureSchema>;
export const storedInteractionSchema = z.looseObject({
  kind: z.string(),
  state: jsonObjectSchema,
  request: interactionRequestSchema.nullable().default(null),
});
export const storedRowSchema = z.looseObject({
  requestId: z.string(),
  createdAt: z.string(),
  record: storedInteractionSchema,
});
export const interactionSnapshotSchema = z.looseObject({
  database: z.string(),
  revision: exactIntegerSchema,
  records: z.array(storedRowSchema),
});
export type InteractionSnapshot = z.infer<typeof interactionSnapshotSchema>;
export const nativeContentSchema = z.looseObject({
  type: z.string(),
  text: z.string().nullable().default(null),
});
export const nativeItemSchema = z.looseObject({
  id: z.string(),
  type: z.string(),
  clientId: z.string().nullable().default(null),
  content: z.array(nativeContentSchema).default([]),
});
export const nativeTurnSchema = z.looseObject({ id: z.string(), items: z.array(nativeItemSchema) });
export const nativeThreadSchema = z.looseObject({
  id: z.string(),
  turns: z.array(nativeTurnSchema).default([]),
});
export const nativeExpectationSchema = z.object({
  thread_id: z.string(),
  turn_id: z.string(),
  push_id: z.string(),
});
export type NativeExpectation = z.infer<typeof nativeExpectationSchema>;
export const nativeMatchSchema = z.object({
  nativeId: z.string(),
  clientId: z.string(),
  turnId: z.string(),
  textSHA256: z.string(),
});
export const nativeCorrelationSchema = z.object({
  threadId: z.string(),
  matchingCount: z.number().int(),
  matches: z.array(nativeMatchSchema),
  supported: z.boolean(),
});
export type NativeCorrelation = z.infer<typeof nativeCorrelationSchema>;
export const signalFailureSchema = z.object({ pid: z.number().int(), error: z.string() });
export const signalResultSchema = z.object({
  sent: z.array(z.number().int()).default([]),
  errors: z.array(signalFailureSchema).default([]),
});
export type SignalResult = z.infer<typeof signalResultSchema>;
export const cleanupResultSchema = z.object({
  exit: z.number().int().nullable(),
  signals: z.array(z.object({ pid: z.number().int(), signal: z.number().int() })),
  errors: z.array(
    z.object({
      stage: z.string().nullable().optional(),
      pid: z.number().int().nullable().optional(),
      error: z.string(),
    }),
  ),
  remaining: z.record(z.string(), z.union([identityRecordSchema, z.string()])),
});

export type CleanupResult = z.infer<typeof cleanupResultSchema>;
export type CleanupFailure = CleanupResult["errors"][number];
export type ProcessSignal = CleanupResult["signals"][number];

export type RequestedInteraction = z.infer<typeof requestedInteractionSchema>;
