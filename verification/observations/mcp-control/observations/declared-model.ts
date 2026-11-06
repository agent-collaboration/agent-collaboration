// The shared exact-target model assertion for MCP and unchanged-CLI verification.
import { z } from "zod";
import { sameSession, sessionRefSchema, type SessionRef } from "./observation-models.ts";

export const declaredProviderSelectionSchema = z.strictObject({
  model: z.string().trim().min(1),
  selectOnCreate: z.boolean(),
  requiredOptions: z.record(z.string().min(1), z.string()),
});
export type DeclaredProviderSelection = z.infer<typeof declaredProviderSelectionSchema>;

const modelInspectSchema = z.looseObject({
  target: sessionRefSchema,
  settingsCatalog: z.looseObject({
    configOptions: z.array(
      z.looseObject({
        id: z.string(),
        category: z.string().optional(),
        currentValue: z.discriminatedUnion("kind", [
          z.object({ kind: z.literal("select"), value: z.string() }),
          z.object({ kind: z.literal("boolean"), value: z.boolean() }),
        ]),
      }),
    ),
  }),
});

export function requireSelectedModel(
  answer: unknown,
  target: SessionRef,
  selection: DeclaredProviderSelection,
): void {
  const inspected = modelInspectSchema.parse(answer);
  if (!sameSession(inspected.target, target))
    throw new Error("model inspect names another Session");
  const models = inspected.settingsCatalog.configOptions.filter(
    (option) => option.category === "model",
  );
  const selected = models.length === 1 ? models[0]?.currentValue : undefined;
  if (selected?.kind !== "select") throw new Error("model inspect has no unique selected model");
  if (selected.value !== selection.model)
    throw new Error(
      `current model ${selected.value} differs from declared model ${selection.model}`,
    );
  for (const [optionId, expectedValue] of Object.entries(selection.requiredOptions)) {
    const matching = inspected.settingsCatalog.configOptions.filter(
      (option) => option.id === optionId,
    );
    const currentValue = matching.length === 1 ? matching[0]?.currentValue : undefined;
    if (currentValue?.kind !== "select")
      throw new Error(`required option ${optionId} has no unique selected value`);
    if (currentValue.value !== expectedValue)
      throw new Error(`required option ${optionId} is ${currentValue.value}, not ${expectedValue}`);
  }
}
