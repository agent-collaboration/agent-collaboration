// The shared exact-target model assertion for MCP and unchanged-CLI verification.
import { z } from "zod";
import { sameSession, sessionRefSchema, type SessionRef } from "./observation-models.ts";

const modelInspectSchema = z.looseObject({
  target: sessionRefSchema,
  settingsCatalog: z.looseObject({
    configOptions: z.array(
      z.looseObject({
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
  declaredModel: string,
): void {
  const inspected = modelInspectSchema.parse(answer);
  if (!sameSession(inspected.target, target))
    throw new Error("model inspect names another Session");
  const models = inspected.settingsCatalog.configOptions.filter(
    (option) => option.category === "model",
  );
  const selected = models.length === 1 ? models[0]?.currentValue : undefined;
  if (selected?.kind !== "select") throw new Error("model inspect has no unique selected model");
  if (selected.value !== declaredModel)
    throw new Error(`current model ${selected.value} differs from declared model ${declaredModel}`);
}
