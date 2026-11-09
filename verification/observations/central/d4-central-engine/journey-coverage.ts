/** D4 required observation records from verification.html and its adopted #d4-notes.
 * Completeness is bookkeeping only; it never supplies runtime or artifact qualification.
 */
import { z } from "zod";

export const d4JourneySchema = z.enum([
  "J1",
  "J1b",
  "J1c",
  "J2",
  "J2b",
  "J2c",
  "J3",
  "J3b",
  "J4",
  "J5",
  "J5b",
  "J6",
  "J6b",
  "J7",
  "J7b",
  "J7c",
  "J8",
  "J9",
  "J10",
  "J13",
  "J14",
  "J15",
  "J17",
  "J18",
  "J19",
  "J20",
  "J21",
  "J22",
  "J23",
  "J25",
  "J26",
  "J27",
  "J28",
  "J29",
]);
export type D4Journey = z.infer<typeof d4JourneySchema>;
export type D4Shell = "tokio" | "workerd" | "staging";
interface JourneyRequirements {
  readonly tokio: readonly string[];
  readonly workerd: readonly string[];
  readonly staging: readonly string[];
}
function localRequirements(checks: readonly string[]): JourneyRequirements {
  return { tokio: checks, workerd: checks, staging: [] };
}
function tokioRequirements(checks: readonly string[]): JourneyRequirements {
  return { tokio: checks, workerd: [], staging: [] };
}
const hibernationChecks = [
  "natural-idle",
  "new-construction-on-frame",
  "same-sockets-and-attachments",
  "frame-answered",
];
const retirementChecks = [
  "backoff-to-ten-minutes",
  "cleanup-and-push-given-up",
  "no-alarm-or-window-slot",
  "payload-kept-sender-pending",
  "health-given-up",
  "public-admin-retry-completes",
];
const requirements: Readonly<Record<D4Journey, JourneyRequirements>> = {
  J1: localRequirements([
    "complete-request-kind-inventory",
    "alarms-disabled",
    "each-kind-answered",
  ]),
  J1b: localRequirements([
    "each-due-owner-effect",
    "alarm-only-input",
    "all-outbound-calls-causally-due",
  ]),
  J1c: localRequirements([
    "real-published-answer-held",
    "other-requests-answered-before-release",
    "publication-finishes",
  ]),
  J2: localRequirements([
    "cleared-through-98-and-notices-99-120",
    "one-frame-repairs-stale-alarm",
    "owed-work-finishes",
    "99-110-removed-111-120-kept",
  ]),
  J2b: localRequirements([
    "next-due-fault",
    "exact-alarm-fault-on-frame",
    "exact-alarm-fault-on-alarm",
    "failure-write-fault",
    "healthy-wake-preserved",
    "real-platform-redelivery",
    "row-and-lower-rollback",
  ]),
  J2c: localRequirements([
    "no-prior-alarm",
    "mail-accepted-exact-write-fault-consumed",
    "no-control-or-input-nudge",
    "retry-from-own-wake",
  ]),
  J3: localRequirements([
    "corrupt-a-notice",
    "a-b-and-b-c-custody",
    "b-welcome",
    "a-other-work",
    "public-health-kind-subject",
  ]),
  J3b: localRequirements([
    "registration-started",
    "all-cleanup-fails",
    "due-step-effect",
    "health-cleanup-error",
  ]),
  J4: localRequirements([
    "work-drained",
    "one-hour",
    "zero-alarm-invocations",
    "no-stored-alarm",
    "passive-observation",
  ]),
  J5: tokioRequirements([
    "100-one-batch",
    "singleton-by-t-plus-30s",
    "1200-three-batches",
    "resident-no-second-batch-within-30s",
    "duplicate-alarm",
    "restart-at-most-one-extra-batch",
  ]),
  J5b: localRequirements([
    "alarms-disabled",
    "one-second-frames",
    "due-effect-on-frame-within-interval",
  ]),
  J6: localRequirements([
    "b-offline-ten-submits",
    "welcome-before-push",
    "real-router-inbox-once",
    "held-report",
    "terminal-report-status-routing",
  ]),
  J6b: localRequirements([
    "1001-distinct-inbox-items",
    "per-connection-peak-at-most-32",
    "per-sender-central-push-order",
    "other-peer-progress-during-drain",
    "health-backlog-shrinks",
  ]),
  J7: localRequirements([
    "mail-push-cut",
    "project-status-cut",
    "machine-status-cut",
    "directory-summary-cut",
    "claim-settlement-cut",
    "session-close-cut",
    "receiver-effect-before-kill",
    "same-store-reopen",
    "dedup-and-debt-finish",
    "nonrepeatable-effect-reconciled",
  ]),
  J7b: localRequirements([
    "final-status-dropped",
    "no-application-poll-or-later-input",
    "quiet-retry-converges",
    "ack-retires-debt",
    "no-later-retry",
  ]),
  J7c: localRequirements([
    "first-physical-close-fails",
    "quiet-revoked-peer-closes",
    "old-frames-refused",
  ]),
  J8: localRequirements([
    "three-real-failed-attempts-connected",
    "public-stuck-count-age-error",
    "same-health-after-rebuild",
  ]),
  J9: localRequirements([
    "exact-five-central-u6-kinds",
    "register-completes",
    "move-completes",
    "operation-due-drives-alarm",
  ]),
  J10: localRequirements([
    "both-shells-required-case-manifest",
    "no-missing-case",
    "semantic-parity-with-named-platform-differences",
  ]),
  J13: tokioRequirements([
    "quiescent-rebuild-switch-actually-fired",
    "no-active-event-or-call-dropped",
    "effects-window-retry-preserved",
    "per-reconstruction-extra-batch-allowance",
  ]),
  J14: {
    tokio: ["no-event-owned-task-waiter-or-timer"],
    workerd: hibernationChecks,
    staging: hibernationChecks,
  },
  J15: {
    tokio: [],
    workerd: [],
    staging: [
      "two-real-routers",
      "application-polls-disabled",
      "one-real-hour",
      "protocol-ping-pong-links-alive",
      "complete-zero-handler-counts",
    ],
  },
  J17: localRequirements([
    "silent-open-peer",
    "200-accepted-peak-at-most-32",
    "backoff-not-cadence-wakes",
    "no-new-work-while-full",
    "absent-socket-without-close-callback",
    "offline-hour-no-peer-row-alarm",
  ]),
  J18: tokioRequirements([
    "earlier-commit-versus-recompute",
    "step-lower-versus-recompute",
    "overlapping-claim",
    "earliest-alarm-no-stale-write",
    "no-duplicate-batch",
  ]),
  J19: localRequirements([
    "corrupt-expired-notice",
    "quiet-backoff-not-ticks",
    "durable-error-first-failure-after-rebuild",
    "healthy-peer-unaffected",
  ]),
  J20: localRequirements([
    "quiet-summary-retry",
    "quiet-close-retry",
    "quiet-settlement-retry",
    "no-inbound-or-socket-nudge",
    "actual-effects-read-back",
  ]),
  J21: localRequirements([
    "silent-holder-timeout-retry",
    "noncommitting-copy-ack-timeout",
    "other-frames-answer",
    "no-leftover-event-timer",
    "active-wait-case-disposition",
  ]),
  J22: localRequirements([
    "actual-old-0134-source-artifact-pair",
    "100-retained-notices-50-mail-single-pushes",
    "peak-at-most-32",
    "one-real-hour-no-unsolicited-notice-resend",
    "passive-alarm-cause-no-notice-wakes",
  ]),
  J23: localRequirements([
    "two-publications-out-of-order",
    "rebuild-after-send-event",
    "replaced-connection-answer-stale",
    "older-revision-answer-stale",
    "correct-row-only-closes",
    "forwarded-holder-answer",
    "stored-holder-reply-drive-one-resume",
  ]),
  J25: localRequirements([
    "silent-other-move",
    "register-and-claim-answer-before-move-deadline",
    "current-wire-result",
  ]),
  J26: {
    tokio: ["24h-injected-history", ...retirementChecks],
    workerd: ["aged-row-at-boundary-not-24h-history", ...retirementChecks],
    staging: [],
  },
  J27: localRequirements([
    "failed-prune-board-and-workspace-writes",
    "cursor-at-exclusive-floor-valid",
    "before-floor-snapshot",
    "after-floor-valid-no-gap",
    "none-expired",
    "all-expired",
    "no-expired-row-served",
    "later-atomic-contiguous-prune",
  ]),
  J28: localRequirements([
    "32-item-batch-per-item-send-id",
    "mailbox-full-item-alone-retries",
    "mixed-type-batch-refused",
    "2MiB-item-goes-alone",
    "released-router-single-pushes",
    "agent-array-order-owned-by-D5",
  ]),
  J29: localRequirements([
    "applied40-issued41-through45",
    "miss43-without-poll",
    "backoff-resends-from43",
    "applied45-retires-attempt",
    "lower-ack-monotonic",
    "past-head-ack-clamped",
    "released-no-owed-row-or-resend",
  ]),
};
const completionChecks = [
  "real-entry-observation",
  "wrong-build-semantic-failure",
  "restored-positive",
  "owned-cleanup",
];
export function requiredD4Checks(shell: D4Shell): readonly string[] {
  const selected: string[] = [];
  for (const journey of d4JourneySchema.options) {
    const checks = requirements[journey][shell];
    if (checks.length === 0) continue;
    selected.push(
      ...[...checks, ...completionChecks].map((check): string => `${journey}:${check}`),
    );
    if (shell === "tokio" && journey !== "J13") selected.push(`J13:rebuild-${journey}`);
  }
  return selected;
}

export const d4CheckOutcomeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("observed"), evidence: z.array(z.string().min(1)).min(1) }).strict(),
  z.object({ kind: z.literal("failed"), reason: z.string().min(1) }).strict(),
  z.object({ kind: z.literal("open"), reason: z.string().min(1) }).strict(),
]);
export type D4CheckOutcome = z.infer<typeof d4CheckOutcomeSchema>;
export interface RecordedD4Coverage {
  readonly runtimeQualified: false;
  readonly complete: boolean;
  readonly observed: readonly string[];
  readonly failed: readonly { readonly check: string; readonly reason: string }[];
  readonly open: readonly { readonly check: string; readonly reason: string }[];
}
export function assessRecordedD4Coverage(input: {
  readonly shell: D4Shell;
  readonly checks: ReadonlyMap<string, D4CheckOutcome>;
}): RecordedD4Coverage {
  const required = requiredD4Checks(input.shell);
  for (const key of input.checks.keys())
    if (!required.includes(key))
      throw new Error(`unregistered D4 check for ${input.shell}: ${key}`);
  const observed: string[] = [];
  const failed: { check: string; reason: string }[] = [];
  const open: { check: string; reason: string }[] = [];
  for (const key of required) {
    const raw = input.checks.get(key);
    if (raw === undefined) {
      open.push({ check: key, reason: "no recorded observation" });
      continue;
    }
    const outcome = d4CheckOutcomeSchema.parse(raw);
    switch (outcome.kind) {
      case "observed":
        observed.push(key);
        break;
      case "failed":
        failed.push({ check: key, reason: outcome.reason });
        break;
      case "open":
        open.push({ check: key, reason: outcome.reason });
        break;
      default:
        assertNever(outcome);
    }
  }
  return {
    runtimeQualified: false,
    complete: failed.length === 0 && open.length === 0,
    observed,
    failed,
    open,
  };
}
function assertNever(value: never): never {
  throw new Error(`unhandled D4 observation outcome: ${String(value)}`);
}
