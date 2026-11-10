/** Exact old2636806a Directory wire, from fleet-admission/directory_query.rs and lease_projection.rs. */
import { z } from "zod";
import { directoryEntry, directoryPage, identifier, unsigned } from "./contracts.ts";

const lease = z.discriminatedUnion("kind", [
  z
    .object({
      kind: z.literal("held"),
      value: z
        .object({
          epoch: unsigned,
          holder: identifier,
          state: z.enum(["held", "expired"]),
          expires_at: z.string(),
        })
        .strict(),
    })
    .strict(),
  z.object({ kind: z.literal("unheld") }).strict(),
  z
    .object({ kind: z.literal("retired"), value: z.object({ retired_at: z.string() }).strict() })
    .strict(),
]);
export const oldDirectoryEntry = directoryEntry
  .omit({ leaseSide: true })
  .extend({
    leaseSide: z.discriminatedUnion("kind", [
      z
        .object({
          kind: z.literal("known"),
          value: z
            .object({
              lease,
              leaseVersion: z
                .object({
                  epoch: unsigned,
                  incarnation: unsigned.refine((value): boolean => value > 0n),
                  change: unsigned.refine((value): boolean => value > 0n),
                })
                .strict(),
              asOf: z.string(),
            })
            .strict(),
        })
        .strict(),
      z.object({ kind: z.literal("notYetReported") }).strict(),
    ]),
  })
  .strict();
export const oldDirectoryPage = directoryPage
  .omit({ projects: true })
  .extend({ projects: z.array(oldDirectoryEntry) })
  .strict();
