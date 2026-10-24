import { z } from "zod";

export const causeSchema = z
  .object({
    name: z.enum([
      "Error",
      "TypeError",
      "AggregateError",
      "AbortError",
      "OtherError",
      "UnavailableOrTruncated",
    ]),
    code: z
      .string()
      .regex(/^[A-Z][A-Z0-9_]{1,40}$/)
      .nullable(),
    get cause(): z.ZodOptional<typeof causeSchema> {
      return causeSchema.optional();
    },
    get errors(): z.ZodOptional<z.ZodArray<typeof causeSchema>> {
      return z.array(causeSchema).max(4).optional();
    },
  })
  .strict();
const stageSchema = z.enum(["version-upload", "deployment-activation", "client-request"]);
export const capturedCauseSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("limit"), messagesRetained: z.literal(false) }).strict(),
  z
    .object({
      kind: z.literal("error"),
      stage: stageSchema,
      cause: causeSchema,
      messagesRetained: z.literal(false),
    })
    .strict(),
  z
    .object({
      kind: z.literal("http"),
      stage: stageSchema,
      status: z.number().int().min(100).max(599),
      messagesRetained: z.literal(false),
    })
    .strict(),
]);
type Cause = z.infer<typeof causeSchema>;
export function sanitize(error: unknown, seen: Set<unknown>, depth = 0): Cause {
  if (!(error instanceof Error) || seen.has(error) || depth >= 8)
    return { name: "UnavailableOrTruncated", code: null };
  seen.add(error);
  const code = z.object({ code: causeSchema.shape.code.optional() }).safeParse(error);
  const name = causeSchema.shape.name.safeParse(error.name);
  const result: Cause = {
    name: name.success ? name.data : "OtherError",
    code: code.success ? (code.data.code ?? null) : null,
  };
  if (error.cause !== undefined) result.cause = sanitize(error.cause, seen, depth + 1);
  if (error instanceof AggregateError)
    result.errors = error.errors
      .slice(0, 4)
      .map((child: unknown): Cause => sanitize(child, seen, depth + 1));
  return result;
}
