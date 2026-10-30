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
  // J3 active-turn preservation: read while the TUI's own turn is still running.
  z.strictObject({ kind: z.literal("turn-in-progress") }),
]);
export type JourneyCheck = z.infer<typeof journeyCheckSchema>;

/**
 * `idle-bind` resumes on its own connection to bind the cwd, which has lifecycle effects, so it
 * runs only once the TUI's step has reached an idle checkpoint. `active-read-only` never resumes
 * or lists skills, so it cannot disturb the active turn it reads.
 */
export const journeyModeSchema = z.enum(["idle-bind", "active-read-only"]);
export type JourneyMode = z.infer<typeof journeyModeSchema>;

export const journeyObservationPlanSchema = metadataObservationPlanSchema
  .omit({
    terminal_capture: true,
    resume_capture: true,
    expected_reply: true,
    require_usage: true,
  })
  .extend({
    // The raw provider SessionRef session ID, addressed by public events and skill references.
    session_id: z.string().min(1),
    // Its native thread ID, the one Codex resumes; checked against the Router alias rule.
    native_thread_id: z.uuid(),
    // The Session's own working directory, which native thread/read and resume must report. The
    // TUI may launch Codex in another trusted directory; idle-bind lists skills and resumes here.
    cwd: absolutePathSchema,
    mode: journeyModeSchema,
    checks: z.array(journeyCheckSchema).min(1),
  })
  .strict()
  .refine(
    (plan) =>
      plan.checks.every(
        (check) => (check.kind === "turn-in-progress") === (plan.mode === "active-read-only"),
      ),
    "active-read-only allows only turn-in-progress, which idle-bind cannot observe",
  );
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
  // The Session's own working directory (v2/thread_data.rs Thread.cwd); a TUI may launch elsewhere.
  cwd: z.string(),
  status: z.looseObject({ type: z.string() }),
  turns: z.array(
    z.looseObject({
      id: z.string(),
      status: z.string(),
      items: z.array(nativeThreadItemSchema),
    }),
  ),
});
export type NativeThread = z.infer<typeof nativeThreadSchema>;

/** What an `active-read-only` observation may collect: history and public events, nothing more. */
export interface ActiveJourneyReadback {
  readonly threadRead: NativeThread;
}

/** The native and public views one `idle-bind` observer connection retains for every check. */
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
