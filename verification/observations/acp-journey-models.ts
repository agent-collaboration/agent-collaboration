import { isAbsolute } from "node:path";
import { z } from "zod";
import { metadataObservationPlanSchema } from "./acp-metadata-models.ts";

/** Native Codex 0.165 `SkillScope` wire values (app-server-protocol v2/plugin.rs, snake_case). */
export const nativeSkillScopeSchema = z.enum(["user", "repo", "system", "admin"]);
export type NativeSkillScope = z.infer<typeof nativeSkillScopeSchema>;

const absolutePathSchema = z.string().refine(isAbsolute, "absolute path required");

/** One provider skill the adopted fixture setup makes invocable in this conversation. */
export const expectedSkillSchema = z.strictObject({
  invocation_name: z.string().min(1),
  scope: nativeSkillScopeSchema,
});
export type ExpectedSkill = z.infer<typeof expectedSkillSchema>;

/**
 * One keyboard submission the TUI journey made. The effect marker is produced only by the
 * fixture skill's own instructions, so a reply containing it cannot be explained by echo.
 */
export const skillSubmissionSchema = z
  .strictObject({
    original_text: z.string().min(1),
    selected: z.array(z.string().min(1)).min(1),
    image_count: z.number().int().nonnegative(),
    effect_marker: z.string().min(8),
  })
  .refine(
    (submission) =>
      !submission.original_text.includes(submission.effect_marker) &&
      !submission.selected.some((name) => name.includes(submission.effect_marker)),
    "effect marker appears in the literal input; an echo could satisfy it",
  )
  .refine(
    (submission) =>
      submission.selected.every((name) => submission.original_text.includes(`$${name}`)),
    "every selected skill must appear as $name in the original text",
  );
export type SkillSubmission = z.infer<typeof skillSubmissionSchema>;

/** Supporting readbacks a TUI journey step needs, decided by Specification C1-C3 and U4. */
export const journeyCheckSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("skills-listed"),
    expected_skills: z.array(expectedSkillSchema).min(1),
    // Advertised provider commands that must never be listed as skills (C1 command spoof).
    excluded_names: z.array(z.string().min(1)).min(1),
  }),
  z.strictObject({ kind: z.literal("skill-effect"), submission: skillSubmissionSchema }),
  z.strictObject({ kind: z.literal("compacted") }),
  z.strictObject({
    kind: z.literal("compact-refused"),
    // A thread/read response retained by an earlier run, before the refused /compact.
    before_thread_read: absolutePathSchema,
  }),
  z.strictObject({ kind: z.literal("effective-model"), expected_model: z.string().min(1) }),
]);
export type JourneyCheck = z.infer<typeof journeyCheckSchema>;

export const journeyObservationPlanSchema = metadataObservationPlanSchema
  .omit({
    terminal_capture: true,
    resume_capture: true,
    expected_reply: true,
    require_usage: true,
  })
  .extend({
    // The working directory the TUI bound with `--cd`; this connection binds it again.
    cwd: absolutePathSchema,
    checks: z.array(journeyCheckSchema).min(1),
  })
  .strict();
export type JourneyObservationPlan = z.infer<typeof journeyObservationPlanSchema>;

/** Used fields of native `skills/list` entries; the raw response stays authoritative. */
export const nativeSkillsListSchema = z.looseObject({
  data: z.array(
    z.looseObject({
      cwd: z.string(),
      skills: z.array(
        z.looseObject({
          name: z.string(),
          path: z.string(),
          scope: z.string(),
          enabled: z.boolean(),
        }),
      ),
      errors: z.array(z.looseObject({ path: z.string(), message: z.string() })),
    }),
  ),
});
export type NativeSkillsList = z.infer<typeof nativeSkillsListSchema>;

export const nativeUserInputSchema = z.looseObject({
  type: z.string(),
  text: z.string().optional(),
});
export const nativeThreadItemSchema = z.looseObject({
  id: z.string(),
  type: z.string(),
  text: z.string().nullable().optional(),
  content: z.array(nativeUserInputSchema).optional(),
});
export type NativeThreadItem = z.infer<typeof nativeThreadItemSchema>;
export const nativeThreadSchema = z.looseObject({
  id: z.string(),
  turns: z.array(
    z.looseObject({
      id: z.string(),
      status: z.string(),
      items: z.array(nativeThreadItemSchema),
    }),
  ),
});
export type NativeThread = z.infer<typeof nativeThreadSchema>;

/** The native and public views one observer connection retains for every check. */
export interface JourneyReadback {
  readonly unboundSkills: NativeSkillsList;
  readonly boundSkills: NativeSkillsList;
  readonly threadRead: NativeThread;
  readonly threadResume: NativeThread;
  readonly resumedModel: string;
  readonly providerAgentTexts: readonly string[];
  readonly providerToolItems: readonly ProviderToolItem[];
}

export interface ProviderToolItem {
  readonly itemId: string;
  readonly toolKind: string;
  readonly status: string;
}

/** Events from public `events observe`, with tool-call kind fields the metadata view drops. */
export const providerJourneyEventsSchema = z.looseObject({
  target: z.looseObject({
    sessionId: z.string(),
    endpoint: z.looseObject({ serviceId: z.string(), endpointId: z.string() }),
  }),
  endReason: z.string(),
  events: z.array(
    z.looseObject({
      event: z.looseObject({
        kind: z.string(),
        item: z
          .looseObject({
            itemId: z.string(),
            kind: z.looseObject({
              kind: z.string(),
              toolKind: z.string().optional(),
              status: z.string().optional(),
            }),
            text: z.string().nullable().default(null),
          })
          .nullable()
          .default(null),
      }),
    }),
  ),
});
export type ProviderJourneyEvents = z.infer<typeof providerJourneyEventsSchema>;

export interface JourneyCheckResult {
  readonly kind: JourneyCheck["kind"];
  readonly observed: Readonly<Record<string, unknown>>;
}
