import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { evidenceJson } from "../evidence-json.ts";
import type { NativeThread } from "./acp-journey-models.ts";

/**
 * Managed single-Session isolation for the Claude route window. The audit has no Session join, so
 * the window is attributed only while the Host holds exactly the declared hosted Claude Session.
 * That is observed through the existing read-only provider Session inventory (collaboration-service
 * provider_session_inventory.rs:163-414), once at the fixed pre-Enter hook and once after the
 * completed reply readback. An extra same-Session turn is rejected from the existing retained
 * native history. A native Claude launch against the same private listener and token is not
 * observable here; keeping that listener unshared stays an allocation premise.
 */

/** The bound normal-build collaboration CLI and the exact declared service and Session it reads. */
export interface ManagedInventoryTarget {
  readonly collaborationCli: { readonly path: string; readonly sha256: string };
  readonly serviceDirectory: string;
  readonly serviceId: string;
  readonly serviceEpoch: string;
  readonly sessionId: string;
}

// The page's ObservationTimestamp (collaboration-protocol endpoint_description.rs:52-67): chrono's
// strict RFC3339 date-time (T, t or space separator, any fractional digits) that ends in Z.
const OBSERVATION_TIMESTAMP = /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/u;
const DAYS_IN_MONTH = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
function isObservationTimestamp(value: string): boolean {
  const fields = OBSERVATION_TIMESTAMP.exec(value)?.slice(1).map(Number);
  if (fields === undefined) return false;
  const [year = 0, month = 0, day = 0, hour = 0, minute = 0, second = 0] = fields;
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = month === 2 && leap ? 29 : (DAYS_IN_MONTH[month - 1] ?? 0);
  // RFC3339 admits a leap second, 60.
  return day >= 1 && day <= days && hour <= 23 && minute <= 59 && second <= 60;
}
const observationTimestampSchema = z
  .string()
  .refine(isObservationTimestamp, "not an RFC3339 UTC observation time");

/** The safe projection kept from one inventory read: identity, state, count and time only. */
export const managedInventoryObservationSchema = z.strictObject({
  service_id: z.string().min(1),
  endpoint_id: z.literal("claude-local"),
  session_id: z.string().min(1),
  state: z.literal("idle"),
  count: z.literal(1),
  observed_at: observationTimestampSchema,
});
export type ManagedInventoryObservation = z.infer<typeof managedInventoryObservationSchema>;

// The one exact read: stored view, all sources, any scope, one full page, no query and no cursor
// (agent-collaboration session_inventory_commands.rs:67-96,160-226). For claude-local this reads
// managed records only and skips native Claude discovery (provider_session_inventory.rs:229-233).
export function managedInventoryArguments(serviceDirectory: string): string[] {
  return [
    "sessions",
    "list",
    "--endpoint",
    "claude-local",
    "--view",
    "stored",
    "--source",
    "all",
    "--any",
    "--page-size",
    "100",
    "--service-directory",
    serviceDirectory,
    "--json",
  ];
}
const INVENTORY_TIMEOUT_MS = 20_000;
const MAX_INVENTORY_OUTPUT_BYTES = 262_144;

const serviceManifestSchema = z.looseObject({ serviceId: z.string(), serviceEpoch: z.string() });

/** The declared service is still the same running instance: its manifest service and epoch. */
export function requireDeclaredService(target: ManagedInventoryTarget): void {
  let manifest;
  try {
    manifest = serviceManifestSchema.parse(
      JSON.parse(readFileSync(join(target.serviceDirectory, "service.json"), "utf8")),
    );
  } catch {
    throw new Error("declared service manifest is unavailable");
  }
  if (manifest.serviceId !== target.serviceId || manifest.serviceEpoch !== target.serviceEpoch)
    throw new Error("the declared service was restarted or replaced");
}

// The CLI page (endpoint_commands.rs:12-77 renames sessions to records and fills nextCursor).
const inventoryPageSchema = z.looseObject({
  kind: z.literal("result"),
  result: z.looseObject({
    page: z.looseObject({
      endpoint: z.looseObject({ serviceId: z.string(), endpointId: z.string() }),
      observedAt: observationTimestampSchema,
      records: z.array(z.unknown()).max(100),
      nextCursor: z.null(),
    }),
  }),
});
// One hosted provider row (provider_session_list.rs:57-68); actor and policy fields are not read.
const hostedRowSchema = z.looseObject({
  origin: z.literal("hostedProvider"),
  target: z.looseObject({
    endpoint: z.looseObject({ serviceId: z.string(), endpointId: z.string() }),
    sessionId: z.string(),
  }),
  state: z.string(),
});

/**
 * Exactly the declared hosted Claude Session, idle, on one complete page of the declared service.
 * A machine success writes exactly one JSON document (session_inventory_commands.rs:228-239), so
 * the whole output must be that one document; any text before or after it is malformed.
 */
export function parseManagedInventory(
  stdout: string,
  target: ManagedInventoryTarget,
): ManagedInventoryObservation {
  let page;
  try {
    page = inventoryPageSchema.parse(JSON.parse(stdout)).result.page;
  } catch {
    throw new Error("managed Session inventory is malformed or incomplete");
  }
  if (Object.hasOwn(page, "skippedRecords") || Object.hasOwn(page, "nativeCoverage"))
    throw new Error("managed Session inventory skipped records or read native sources");
  if (page.endpoint.serviceId !== target.serviceId || page.endpoint.endpointId !== "claude-local")
    throw new Error("managed Session inventory answered for another service or endpoint");
  if (page.records.length !== 1)
    throw new Error(
      `managed Session inventory holds ${page.records.length} Sessions, not exactly one`,
    );
  const row = hostedRowSchema.safeParse(page.records[0]);
  if (!row.success) throw new Error("the managed Session is not one hosted provider record");
  const { target: listed, state } = row.data;
  if (
    listed.endpoint.serviceId !== target.serviceId ||
    listed.endpoint.endpointId !== "claude-local" ||
    listed.sessionId !== target.sessionId
  )
    throw new Error("the managed Session is not the declared target");
  if (state !== "idle") throw new Error("the declared managed Session is not idle");
  return managedInventoryObservationSchema.parse({
    service_id: target.serviceId,
    endpoint_id: "claude-local",
    session_id: target.sessionId,
    state: "idle",
    count: 1,
    observed_at: page.observedAt,
  });
}

/**
 * One bounded inventory read through the exact bound CLI, between two checks that the declared
 * service instance is unchanged. Failures are categorical; raw CLI output is never retained.
 */
export function readManagedInventory(target: ManagedInventoryTarget): ManagedInventoryObservation {
  requireDeclaredService(target);
  let binary: Buffer;
  try {
    binary = readFileSync(target.collaborationCli.path);
  } catch {
    throw new Error("bound collaboration CLI is unavailable");
  }
  if (createHash("sha256").update(binary).digest("hex") !== target.collaborationCli.sha256)
    throw new Error("bound collaboration CLI changed");
  const completed = spawnSync(
    target.collaborationCli.path,
    managedInventoryArguments(target.serviceDirectory),
    { encoding: "utf8", timeout: INVENTORY_TIMEOUT_MS, maxBuffer: MAX_INVENTORY_OUTPUT_BYTES },
  );
  if (completed.error !== undefined) throw new Error("managed Session inventory is unavailable");
  if (completed.status !== 0)
    throw new Error(`managed Session inventory failed with exit ${String(completed.status)}`);
  const observation = parseManagedInventory(completed.stdout, target);
  requireDeclaredService(target);
  return observation;
}

/** The retained before-state is the declared Session's own idle history at its own cwd. */
export function requireHistoryBefore(
  before: NativeThread,
  nativeThreadId: string,
  cwd: string,
): void {
  if (before.id !== nativeThreadId || before.cwd !== cwd || before.status.type !== "idle")
    throw new Error("the retained before-state is not the declared idle Session at its cwd");
}

type Turn = NativeThread["turns"][number];
// The whole observed turn, items and their content included, in the exact evidence serialization
// with sorted keys. Only its digest is retained; the content stays in the existing history artifact.
const turnShape = (turn: Turn): string => evidenceJson(turn, true);

/** A digest of the before-state's complete turns, retained instead of its history. */
export function historyDigest(thread: NativeThread): string {
  return createHash("sha256")
    .update(JSON.stringify(thread.turns.map(turnShape)))
    .digest("hex");
}

/** The final view keeps the before-state's turns unchanged and adds exactly the reply turn. */
export function requireOneNewReplyTurn(
  before: NativeThread,
  after: NativeThread,
  replyTurnId: string,
): void {
  if (after.turns.length !== before.turns.length + 1)
    throw new Error("the Session gained other than exactly one turn since the before-state");
  if (
    before.turns.some((turn, index) => {
      const kept = after.turns[index];
      return kept === undefined || turnShape(turn) !== turnShape(kept);
    })
  )
    throw new Error("the Session's prior turns changed since the before-state");
  const added = after.turns.at(-1);
  if (added?.status !== "completed" || added.id !== replyTurnId)
    throw new Error("the one new turn is not the completed reply turn");
}
