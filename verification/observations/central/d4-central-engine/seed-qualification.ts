/** D4-only semantic comparison of a positive case and its isolated wrong-build receipt.
 * This is a reader, not an attestation: artifact/source custody and live evidence remain separate.
 */
import { resolve, sep } from "node:path";
import { z } from "zod";
import { isDeepStrictEqual } from "node:util";

import { d4JourneySchema } from "./journey-coverage.ts";

const caseIdentity = z.object({
  journey: d4JourneySchema,
  scenario: z.string().min(1),
  shell: z.enum(["tokio", "workerd"]),
});
const receiptSchema = caseIdentity
  .extend({
    sourceRoot: z.string().startsWith("/"),
    baseProductHead: z.string().regex(/^[a-f0-9]{40}$/),
    artifactSHA256: z.string().regex(/^[a-f0-9]{64}$/),
    oracleSHA256: z.string().regex(/^[a-f0-9]{64}$/),
    configurationSHA256: z.string().regex(/^[a-f0-9]{64}$/),
    reached: z.array(z.string().min(1)),
    checked: z.array(z.string().min(1)),
    cleanup: z.enum(["settled", "unverified"]),
    outcome: z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("passed") }).strict(),
      z
        .object({
          kind: z.literal("assertionFailed"),
          assertion: z.string().min(1),
          actual: z.unknown(),
          expected: z.unknown(),
        })
        .strict(),
      z.object({ kind: z.literal("harnessFailed"), error: z.string().min(1) }).strict(),
      z.object({ kind: z.literal("unavailable"), reason: z.string().min(1) }).strict(),
    ]),
  })
  .strict();
const expectedSeedSchema = caseIdentity
  .extend({
    assertion: z.string().min(1),
    readiness: z.array(z.string().min(1)).min(1),
  })
  .strict();
export type SeedFailureAssessment =
  | { readonly kind: "unknown"; readonly reason: string }
  | {
      readonly kind: "semanticFailureMatched";
      readonly assertion: string;
      readonly runtimeQualified: false;
      readonly remaining: readonly string[];
    };

export function assessSeedFailure(input: {
  readonly expected: unknown;
  readonly positive: unknown;
  readonly seeded: unknown;
  readonly restored: unknown;
}): SeedFailureAssessment {
  const expected = expectedSeedSchema.safeParse(input.expected);
  const positive = receiptSchema.safeParse(input.positive);
  const seeded = receiptSchema.safeParse(input.seeded);
  const restored = receiptSchema.safeParse(input.restored);
  if (!expected.success || !positive.success || !seeded.success || !restored.success)
    return { kind: "unknown", reason: "invalid D4 seed declaration or receipt" };
  const oracle = expected.data;
  const control = positive.data;
  const candidate = seeded.data;
  for (const observed of [control, candidate, restored.data]) {
    if (
      observed.journey !== oracle.journey ||
      observed.scenario !== oracle.scenario ||
      observed.shell !== oracle.shell
    )
      return { kind: "unknown", reason: "receipt does not select the declared D4 case and shell" };
    if (oracle.readiness.some((stage): boolean => !observed.reached.includes(stage)))
      return { kind: "unknown", reason: "declared readiness or fault cut was not reached" };
    if (!observed.checked.includes(oracle.assertion))
      return { kind: "unknown", reason: "declared semantic check did not execute" };
    if (observed.cleanup !== "settled")
      return { kind: "unknown", reason: "owned cleanup is unverified" };
    if (resolve(observed.sourceRoot) !== observed.sourceRoot)
      return { kind: "unknown", reason: "source root is not normalized" };
  }
  if (
    control.baseProductHead !== candidate.baseProductHead ||
    control.oracleSHA256 !== candidate.oracleSHA256 ||
    control.configurationSHA256 !== candidate.configurationSHA256
  )
    return { kind: "unknown", reason: "seed comparison changed its base, oracle or configuration" };
  if (
    control.sourceRoot === candidate.sourceRoot ||
    candidate.sourceRoot.startsWith(`${control.sourceRoot}${sep}`) ||
    control.sourceRoot.startsWith(`${candidate.sourceRoot}${sep}`)
  )
    return { kind: "unknown", reason: "seed and positive source roots are not isolated" };
  if (control.artifactSHA256 === candidate.artifactSHA256)
    return { kind: "unknown", reason: "seed artifact is identical to the positive artifact" };
  if (control.outcome.kind !== "passed")
    return { kind: "unknown", reason: "positive control did not pass" };
  if (
    restored.data.outcome.kind !== "passed" ||
    restored.data.artifactSHA256 !== control.artifactSHA256 ||
    restored.data.baseProductHead !== control.baseProductHead ||
    restored.data.oracleSHA256 !== control.oracleSHA256 ||
    restored.data.configurationSHA256 !== control.configurationSHA256
  )
    return {
      kind: "unknown",
      reason: "restored positive did not pass on the same original artifact and inputs",
    };
  if (
    candidate.outcome.kind !== "assertionFailed" ||
    candidate.outcome.assertion !== oracle.assertion
  )
    return { kind: "unknown", reason: "seed did not fail the exact declared semantic assertion" };
  if (isDeepStrictEqual(candidate.outcome.actual, candidate.outcome.expected))
    return { kind: "unknown", reason: "failed assertion contains no differing actual result" };
  return {
    kind: "semanticFailureMatched",
    assertion: oracle.assertion,
    runtimeQualified: false,
    remaining: [
      "source-copy and patch custody",
      "signed artifact provenance",
      "original live observation evidence",
    ],
  };
}
