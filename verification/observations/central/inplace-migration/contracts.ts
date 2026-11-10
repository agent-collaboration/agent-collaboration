/** Independent observation contracts; migration inventory/verify reports are never inputs. */
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";

export const identifier = z.uuid();
export const unsigned = z.bigint().nonnegative().max(18446744073709551615n);
const position = z.object({ epoch: unsigned, sequence: unsigned }).strict();
export const copyStatus = z
  .object({
    database: identifier,
    role: z.enum(["authoritative", "follower"]),
    fence: unsigned,
    holder: identifier.nullable(),
    start: position,
    acked: position,
    schema: z.object({ version: unsigned, fingerprint: z.string().min(1) }).strict(),
  })
  .strict();
const heldLease = z.discriminatedUnion("kind", [
  z
    .object({
      kind: z.literal("held"),
      value: z
        .object({
          epoch: unsigned,
          holder: identifier,
          state: z.enum(["held", "expired"]),
          expiresAt: z.string(),
        })
        .strict(),
    })
    .strict(),
  z.object({ kind: z.literal("unheld") }).strict(),
  z
    .object({ kind: z.literal("retired"), value: z.object({ retiredAt: z.string() }).strict() })
    .strict(),
]);
export const directoryEntry = z
  .object({
    project: identifier,
    name: z.string(),
    workspace: identifier,
    board: identifier,
    boardArchive: z.enum(["open", "closed", "archiving", "archived"]),
    copySide: z.unknown(),
    leaseSide: z.discriminatedUnion("kind", [
      z
        .object({
          kind: z.literal("known"),
          value: z.object({ lease: heldLease, leaseVersion: unsigned, asOf: z.string() }).strict(),
        })
        .strict(),
      z.object({ kind: z.literal("notYetReported") }).strict(),
    ]),
  })
  .strict();
export const directoryPage = z
  .object({ projects: z.array(directoryEntry), nextAfter: identifier.nullable(), asOf: z.string() })
  .strict();
function known<TSchema extends z.ZodType>(value: TSchema) {
  return z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("known"), value }).strict(),
    z
      .object({ kind: z.literal("unreadable"), value: z.object({ detail: z.string() }).strict() })
      .strict(),
  ]);
}
export const healthReply = z.discriminatedUnion("kind", [
  z
    .object({
      kind: z.literal("live"),
      value: z
        .object({
          bookkeeping: z.unknown(),
          migrationFailures: z.unknown(),
          stage: known(z.enum(["starting", "seeding", "following"]).nullable()),
          copy: known(copyStatus.nullable()),
          projectFingerprint: known(z.string().min(1)),
          stagedParts: z.unknown(),
          storageBytes: unsigned,
        })
        .strict(),
    })
    .strict(),
  z
    .object({ kind: z.literal("retired"), value: z.object({ retiredAt: z.string() }).strict() })
    .strict(),
]);
export const source = z
  .object({
    artifact: z.string().min(1),
    sha256: z.string().regex(/^[a-f0-9]{64}$/),
    method: z.enum([
      "centralGet",
      "routerPublicRead",
      "retainedWriterRead",
      "machineAuthentication",
    ]),
  })
  .strict();
const namedRecord = z.object({ id: identifier, facts: z.record(z.string(), z.unknown()) }).strict();
export const routerTarget = z
  .object({
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    version: z.string().regex(/^\d+\.\d+\.\d+$/),
  })
  .strict();
export const routerObservation = z
  .object({
    machineLabel: z.enum(["sunbook", "sunclaw"]),
    machineId: identifier,
    installationId: identifier,
    routerVersion: routerTarget.shape.version,
    build: z
      .object({
        sourceHead: routerTarget.shape.sourceHead,
        router: artifactSchema,
        cli: artifactSchema,
      })
      .strict(),
    sources: z.array(source).min(1),
    listsComplete: z.boolean(),
    workspaces: z.array(namedRecord),
    boards: z.array(namedRecord),
    folders: z.array(namedRecord),
    workspaceAdrs: z.array(namedRecord),
    workspaceGraphComplete: z.boolean(),
    writers: z.array(
      z.object({ database: identifier, holder: identifier, lease: unsigned }).strict(),
    ),
    writerReadComplete: z.boolean(),
    content: z.array(
      z
        .object({
          project: identifier,
          readerInstallation: identifier,
          holderInstallation: identifier.nullable(),
          complete: z.literal(true),
          records: z.array(namedRecord).min(1),
          source: source.refine((value): boolean => value.method === "routerPublicRead"),
        })
        .strict()
        .refine(
          (value): boolean => value.readerInstallation !== value.holderInstallation,
          "content must be read by the non-holder",
        ),
    ),
    authentication: z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("notObserved") }).strict(),
      z
        .object({
          kind: z.literal("observed"),
          connected: z.literal(true),
          enrollInvocations: z.literal(0n),
          renewed: z.array(
            z.object({ database: identifier, holder: identifier, lease: unsigned }).strict(),
          ),
          source: source.refine((value): boolean => value.method === "machineAuthentication"),
        })
        .strict(),
    ]),
  })
  .strict();
export const copyObservation = z.object({ project: identifier, health: healthReply }).strict();
export const snapshot = z
  .object({
    format: z.literal("central-inplace-observation-v1"),
    runId: z.uuidv7(),
    central: z.object({ origin: z.url(), sourceHead: z.string().regex(/^[a-f0-9]{40}$/) }).strict(),
    phase: z.enum(["before", "after"]),
    routerTarget,
    routerProjects: z
      .array(identifier)
      .min(1)
      .refine(
        (projects): boolean => new Set(projects).size === projects.length,
        "unique declared Router project membership required",
      ),
    capturedAt: z.iso.datetime(),
    directory: z.array(directoryEntry).min(1),
    copies: z.array(copyObservation).min(1),
    sources: z.array(source).min(1),
    routers: z.array(routerObservation),
    limitations: z.array(z.string()),
  })
  .strict();
export type Snapshot = z.infer<typeof snapshot>;
export type RouterObservation = z.infer<typeof routerObservation>;
export type HealthReply = z.infer<typeof healthReply>;
export type Source = z.infer<typeof source>;
export const capturePlan = z
  .object({
    centralOrigin: z.url(),
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    phase: z.enum(["before", "after"]),
    routerTarget,
    routerProjects: z
      .array(identifier)
      .min(1)
      .refine(
        (projects): boolean => new Set(projects).size === projects.length,
        "unique declared Router project membership required",
      ),
    evidenceDirectory: z.string().startsWith("/"),
    ownerAssertionEnv: z.string().regex(/^[A-Z][A-Z0-9_]*$/),
    routerObservationFiles: z.array(z.string().startsWith("/")),
  })
  .strict();
export type CapturePlan = z.infer<typeof capturePlan>;
