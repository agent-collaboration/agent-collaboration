import * as z from "zod/mini";

const identifierSchema = z.string().check(
  z.minLength(1),
  z.refine((value) => !value.includes("\0")),
);
const reasonSchema = z.enum([
  "UnsupportedSource",
  "Unavailable",
  "Malformed",
  "Ambiguous",
  "Unrecognized",
]);
const choiceSchema = z.object({ value: identifierSchema, label: identifierSchema });
const modelSchema = z.discriminatedUnion("kind", [
  z.object({ modelId: identifierSchema, kind: z.literal("unknown"), reason: reasonSchema }),
  z
    .object({
      modelId: identifierSchema,
      kind: z.literal("select"),
      configId: identifierSchema,
      choices: z.array(choiceSchema),
      defaultValue: identifierSchema,
    })
    .check(
      z.refine(
        (row) =>
          row.choices.length > 0 &&
          row.choices.some((choice) => choice.value === row.defaultValue) &&
          new Set(row.choices.map((choice) => choice.value)).size === row.choices.length,
      ),
    ),
]);
export const modelReasoningSchema = z.discriminatedUnion("kind", [
  z.object({ version: z.literal(1), kind: z.literal("unknown"), reason: reasonSchema }),
  z
    .object({ version: z.literal(1), kind: z.literal("available"), models: z.array(modelSchema) })
    .check(
      z.refine(
        (view) => new Set(view.models.map((model) => model.modelId)).size === view.models.length,
      ),
    ),
]);
type ModelReasoning = z.infer<typeof modelReasoningSchema>;
type ModelRow = z.infer<typeof modelSchema>;
const modelInfoSchema = z.looseObject({
  value: identifierSchema,
  supportsEffort: z.optional(z.boolean()),
  supportedEffortLevels: z.optional(z.array(identifierSchema)),
});
const modelsSchema = z.looseObject({
  currentModelId: identifierSchema,
  availableModels: z.array(z.looseObject({ modelId: identifierSchema })),
});
const metadataSchema = z.record(z.string(), z.unknown());
const resultSchema = z.looseObject({
  sessionId: z.optional(identifierSchema),
  _meta: z.optional(metadataSchema),
  configOptions: z.optional(z.unknown()),
});
const notificationSchema = z.looseObject({
  sessionId: identifierSchema,
  update: z.looseObject({ sessionUpdate: z.string(), _meta: z.optional(metadataSchema) }),
});

export interface ModelMetadataSession {
  readonly sessionIdentity: object;
  readonly queryIdentity: object;
  readonly models: unknown;
  readonly modelInfos: unknown;
  readonly configOptions: unknown;
}
type ResultSource =
  | { readonly kind: "lifecycle" }
  | { readonly kind: "setting"; readonly before: ModelMetadataSession | undefined };
interface ResultDecoration {
  readonly sessionId: string;
  readonly result: unknown;
  readonly source: ResultSource;
}
interface ModelMetadataInputs {
  readonly currentSession: (sessionId: string) => ModelMetadataSession | undefined;
}

function unknown(reason: z.infer<typeof reasonSchema>): ModelReasoning {
  return { version: 1, kind: "unknown", reason };
}

function modelFacts(session: ModelMetadataSession): ModelReasoning {
  const models = modelsSchema.safeParse(session.models);
  const infos = z.array(modelInfoSchema).safeParse(session.modelInfos);
  if (!models.success || !infos.success) return unknown("Malformed");
  const ids = models.data.availableModels.map((model) => model.modelId);
  if (new Set(ids).size !== ids.length) return unknown("Ambiguous");
  const rows = ids.map((modelId): ModelRow => {
    const matching = infos.data.filter((info) => info.value === modelId);
    const info = matching.length === 1 ? matching[0] : undefined;
    if (matching.length > 1) return { modelId, kind: "unknown", reason: "Ambiguous" };
    const levels = info?.supportedEffortLevels;
    if (info?.supportsEffort !== true || levels === undefined || levels.length === 0)
      return { modelId, kind: "unknown", reason: "Unavailable" };
    if (new Set(levels).size !== levels.length || levels.includes("default"))
      return { modelId, kind: "unknown", reason: "Ambiguous" };
    // Qualified stock0.81 legacy buildEffortConfigOption/toSdkEffortLevel:
    // "default" is an offered real reset, independent of saved currentValue.
    return {
      modelId,
      kind: "select",
      configId: "effort",
      defaultValue: "default",
      choices: [
        { value: "default", label: "Default" },
        ...levels.map((value) => ({
          value,
          label: value
            .split(/[_-]/)
            .map((part) => (part ? `${part.charAt(0).toUpperCase()}${part.slice(1)}` : part))
            .join(" "),
        })),
      ],
    };
  });
  return modelReasoningSchema.parse({ version: 1, kind: "available", models: rows });
}

function annotate(
  metadata: Record<string, unknown> | undefined,
  facts: ModelReasoning,
): Record<string, unknown> {
  const previous = metadataSchema.safeParse(metadata?.codexRouter);
  return {
    ...metadata,
    codexRouter: { ...(previous.success ? previous.data : {}), modelReasoning: facts },
  };
}

export class ModelMetadataDecorator {
  readonly #inputs: ModelMetadataInputs;
  constructor(inputs: ModelMetadataInputs) {
    this.#inputs = inputs;
  }

  currentSession(sessionId: string): ModelMetadataSession | undefined {
    return this.#inputs.currentSession(sessionId);
  }

  decorateResult({ sessionId, result, source }: ResultDecoration): unknown {
    const parsed = resultSchema.safeParse(result);
    if (!parsed.success) return result;
    if (parsed.data.sessionId !== undefined && parsed.data.sessionId !== sessionId) return result;
    const session = this.#inputs.currentSession(sessionId);
    const facts =
      session === undefined ||
      !Array.isArray(session.configOptions) ||
      parsed.data.configOptions !== session.configOptions ||
      (source.kind === "setting" &&
        (source.before === undefined ||
          source.before.sessionIdentity !== session.sessionIdentity ||
          source.before.queryIdentity !== session.queryIdentity))
        ? unknown("Unavailable")
        : modelFacts(session);
    return { ...parsed.data, _meta: annotate(parsed.data._meta, facts) };
  }

  decorateNotification(notification: unknown): unknown {
    const parsed = notificationSchema.safeParse(notification);
    if (!parsed.success || parsed.data.update.sessionUpdate !== "config_option_update")
      return notification;
    const session = this.#inputs.currentSession(parsed.data.sessionId);
    const facts =
      session === undefined ||
      !Array.isArray(session.configOptions) ||
      parsed.data.update.configOptions !== session.configOptions
        ? unknown("Unavailable")
        : modelFacts(session);
    return {
      ...parsed.data,
      update: { ...parsed.data.update, _meta: annotate(parsed.data.update._meta, facts) },
    };
  }
}

export function decorateSessionMethods(
  agent: Record<string, unknown>,
  metadata: ModelMetadataDecorator,
): void {
  const sessionAddressSchema = z.looseObject({ sessionId: z.string().check(z.minLength(1)) });
  for (const methodName of [
    "newSession",
    "loadSession",
    "resumeSession",
    "setSessionConfigOption",
  ]) {
    const original = agent[methodName];
    if (typeof original !== "function")
      throw new Error(`Qualified Claude agent lacks ${methodName}`);
    agent[methodName] = async (params: unknown): Promise<unknown> => {
      const address = sessionAddressSchema.safeParse(params);
      const before = address.success ? metadata.currentSession(address.data.sessionId) : undefined;
      const result: unknown = await Reflect.apply(original, agent, [params]);
      const returned = sessionAddressSchema.safeParse(result);
      if (address.success && returned.success && address.data.sessionId !== returned.data.sessionId)
        return result;
      const sessionId = returned.success
        ? returned.data.sessionId
        : address.success
          ? address.data.sessionId
          : undefined;
      const source: ResultSource =
        methodName === "setSessionConfigOption"
          ? { kind: "setting", before }
          : { kind: "lifecycle" };
      return sessionId === undefined
        ? result
        : metadata.decorateResult({ sessionId, result, source });
    };
  }
}
