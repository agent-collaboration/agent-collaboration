/** The corrected D1 VD's observation inventory. Missing checks stay open.
 * Completion of this inventory is evidence for the Lead, never acceptance.
 */
import { z } from "zod";

export const d1ObservationIdSchema = z.enum([
  "O1",
  "O2",
  "O3",
  "O4",
  "O5",
  "O6",
  "O7",
  "O8",
  "O9",
  "O10",
  "O11",
]);
export type D1ObservationId = z.infer<typeof d1ObservationIdSchema>;
export type D1Backend = "tokio" | "staging";
interface ObservationDefinition {
  readonly id: D1ObservationId;
  readonly target: "both" | "staging";
  readonly lifetime: "lasting" | "temporary";
  readonly checks: readonly string[];
}

export const d1Observations: readonly ObservationDefinition[] = [
  {
    id: "O1",
    target: "both",
    lifetime: "lasting",
    checks: [
      "sender-accepted-while-b-offline",
      "recipient-inbox-held",
      "sender-inbox-status",
      "same-recipient-opened",
      "sender-delivered",
      "one-correlated-native-input",
      "a-and-fleet-payload-released",
      "all-holder-v7-and-times",
      "v4-refused-at-ingress",
    ],
  },
  {
    id: "O2",
    target: "both",
    lifetime: "lasting",
    checks: ["exact-fleet-p1-p2-stores", "three-sockets-owned-by-fleet", "no-other-object-sockets"],
  },
  {
    id: "O3",
    target: "both",
    lifetime: "lasting",
    checks: [
      "a-restart-before-fleet-answer",
      "fleet-restart-after-admission",
      "b-restart-after-inbox-before-receipt",
      "fleet-restart-after-receipt-before-status",
      "a-offline-at-status",
      "c-cannot-answer-for-b",
      "handoff-unknown-without-retry",
      "same-id-changed-content-conflict",
      "two-origins-late-and-wrong-holder",
    ],
  },
  {
    id: "O4",
    target: "both",
    lifetime: "lasting",
    checks: [
      "injected-clock-on-all-holders",
      "all-holder-day59-and-day60",
      "age61-refused-at-every-ingress",
    ],
  },
  {
    id: "O5",
    target: "staging",
    lifetime: "temporary",
    checks: [
      "same-url-relay-receiver-reset",
      "kill-at-partial-clear-barrier",
      "restart-clears-before-central-read",
      "new-lease-and-project-write",
      "local-content-outbox-history-and-identity-preserved",
    ],
  },
  {
    id: "O6",
    target: "staging",
    lifetime: "lasting",
    checks: [
      "fixed-n-rate-duration",
      "one-fleet-object-connections",
      "cloudflare-request-rate",
      "cloudflare-cpu-per-request",
      "errors-and-router-push-latency",
    ],
  },
  {
    id: "O7",
    target: "both",
    lifetime: "lasting",
    checks: [
      "persistent-p1-fault-observed",
      "mail-both-directions-during-fault",
      "p2-read-and-write-during-fault",
      "p1-still-failing-before-clear",
    ],
  },
  {
    id: "O8",
    target: "both",
    lifetime: "lasting",
    checks: [
      "actual-fleet-full-schema",
      "concern-prefixes",
      "v7-and-timestamps",
      "json-reasons-and-journal-disposition",
      "boolean-only-check-constraints",
    ],
  },
  {
    id: "O9",
    target: "both",
    lifetime: "lasting",
    checks: [
      "two-real-local-luna-sessions",
      "one-local-inbox-row",
      "no-matching-local-outbox",
      "one-correlated-native-input",
      "local-item-v7-and-times",
    ],
  },
  {
    id: "O10",
    target: "both",
    lifetime: "lasting",
    checks: [
      "twenty-distinct-online-items",
      "accepted-at-minus-created-at",
      "samples-and-median-reported",
    ],
  },
  {
    id: "O11",
    target: "both",
    lifetime: "lasting",
    checks: [
      "never-enrolled-socket-refused",
      "never-enrolled-no-outbox-change",
      "revoked-still-open-submit-refused",
      "revoked-no-outbox-change",
    ],
  },
];

export const d1CheckOutcomeSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("observed"), evidence: z.array(z.string().min(1)).min(1) }).strict(),
  z.object({ kind: z.literal("failed"), reason: z.string().min(1) }).strict(),
  z.object({ kind: z.literal("open"), reason: z.string().min(1) }).strict(),
]);
export type D1CheckOutcome = z.infer<typeof d1CheckOutcomeSchema>;
export type D1ObservationOutcome =
  | { readonly kind: "observed"; readonly evidence: readonly string[] }
  | { readonly kind: "failed"; readonly check: string; readonly reason: string }
  | { readonly kind: "open"; readonly missing: readonly string[]; readonly reason: string };

export function d1CheckKey(observation: D1ObservationId, check: string): string {
  return `${observation}:${check}`;
}

export function assessD1Coverage(input: {
  backend: D1Backend;
  checks: ReadonlyMap<string, D1CheckOutcome>;
}): ReadonlyMap<D1ObservationId, D1ObservationOutcome> {
  const registered = new Set(
    d1Observations.flatMap((row): string[] =>
      row.checks.map((check): string => d1CheckKey(row.id, check)),
    ),
  );
  for (const [key, outcome] of input.checks) {
    if (!registered.has(key)) throw new Error(`unregistered D1 check: ${key}`);
    d1CheckOutcomeSchema.parse(outcome);
  }
  return new Map(
    d1Observations.map((row): [D1ObservationId, D1ObservationOutcome] => {
      if (row.target === "staging" && input.backend === "tokio")
        return [
          row.id,
          {
            kind: "open",
            missing: row.checks,
            reason: "staging-only; declared, not executed on Tokio",
          },
        ];
      const evidence: string[] = [];
      const missing: string[] = [];
      for (const check of row.checks) {
        const outcome = input.checks.get(d1CheckKey(row.id, check));
        if (outcome?.kind === "failed")
          return [row.id, { kind: "failed", check, reason: outcome.reason }];
        if (outcome?.kind === "observed") evidence.push(...outcome.evidence);
        else missing.push(check);
      }
      return missing.length === 0
        ? [row.id, { kind: "observed", evidence: [...new Set(evidence)] }]
        : [
            row.id,
            { kind: "open", missing, reason: "required D1 observations remain unconfirmed" },
          ];
    }),
  );
}
