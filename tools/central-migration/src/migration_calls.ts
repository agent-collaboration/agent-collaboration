// The four migration calls over the five storage objects, apart from the HTTP
// shell that authenticates the owner. Each object changes in its own storage
// transaction, so a call interrupted between objects leaves every object
// either wholly old or wholly migrated; each call below resumes from exactly
// that per-object state, recorded by the object's own migration-record table.

import { parseExact, stringifyExact } from "./exact_json.ts";
import {
  COPY_READ,
  FLEET_READ,
  REGISTRY_READ,
  planMigration,
  type MacFileEvidence,
  type MigrationPlan,
  type OldStorage,
} from "./migration_plan.ts";
import { JOURNAL_REVERT } from "./journal_conversion.ts";
import type { MigrateRequest } from "./mac_file_evidence.ts";
import { rehearseCandidate, type CandidateSource } from "./migration_candidate.ts";
import { COPY_VERIFY_READ, FLEET_VERIFY_READ, verifyMigration } from "./migration_verify.ts";
import type { DurableObjectNamespace } from "./platform.ts";
import {
  COPY_RENAMED,
  FLEET_RENAMED,
  MAIN_FLEET_CREATION_ORDER,
  ledgerTable,
  legacyName,
} from "./schema_sets.ts";
import {
  OBJECT_APPLY_URL,
  OBJECT_DUMP_URL,
  OBJECT_READ_URL,
  OBJECT_REGISTRY_PROJECTS_URL,
  OBJECT_REHEARSE_URL,
  OBJECT_REVERT_URL,
  type ApplyReply,
  type ObjectDump,
  type ReadReply,
  type RehearseReply,
  type RevertReply,
  type Statement,
} from "./storage_objects.ts";

export interface StorageNamespaces {
  readonly LEASE_REGISTRY: DurableObjectNamespace;
  readonly CONNECTIONS: DurableObjectNamespace;
  readonly PROJECT_COPIES: DurableObjectNamespace;
  readonly FLEET: DurableObjectNamespace;
  readonly MAILBOXES: DurableObjectNamespace;
}

/**
 * The internal calls into each object. Object names follow production's
 * locations (O crates/collaboration-central-cloudflare/src/object_location.rs:3–9).
 */
export class StorageObjects {
  readonly namespaces: StorageNamespaces;
  rowsWritten = 0;
  constructor(namespaces: StorageNamespaces) {
    this.namespaces = namespaces;
  }
  private async call<T>(
    namespace: DurableObjectNamespace,
    name: string,
    url: string,
    body?: unknown,
  ): Promise<T> {
    const response = await namespace
      .get(namespace.idFromName(name))
      .fetch(url, body === undefined ? undefined : { method: "POST", body: stringifyExact(body) });
    if (!response.ok)
      throw new Error(`${name}: ${url} answered ${response.status}: ${await response.text()}`);
    const reply = parseExact(await response.text()) as T & { rowsWritten?: number };
    this.rowsWritten += reply.rowsWritten ?? 0;
    return reply;
  }
  registry(tables: readonly string[]) {
    return this.call<ReadReply>(this.namespaces.LEASE_REGISTRY, "registry", OBJECT_READ_URL, {
      tables,
    });
  }
  fleet(tables: readonly string[]) {
    return this.call<ReadReply>(this.namespaces.FLEET, "fleet", OBJECT_READ_URL, { tables });
  }
  copy(project: string, tables: readonly string[]) {
    return this.call<ReadReply>(this.namespaces.PROJECT_COPIES, project, OBJECT_READ_URL, {
      tables,
    });
  }
  dump(namespace: DurableObjectNamespace, name: string) {
    return this.call<ObjectDump>(namespace, name, OBJECT_DUMP_URL);
  }
  projects() {
    return this.call<string[]>(
      this.namespaces.LEASE_REGISTRY,
      "registry",
      OBJECT_REGISTRY_PROJECTS_URL,
    );
  }
  applyFleet(statements: readonly Statement[]) {
    return this.call<ApplyReply>(this.namespaces.FLEET, "fleet", OBJECT_APPLY_URL, {
      refuseIfPresent: ledgerTable("central-fleet"),
      statements,
    });
  }
  applyCopy(project: string, statements: readonly Statement[]) {
    return this.call<ApplyReply>(this.namespaces.PROJECT_COPIES, project, OBJECT_APPLY_URL, {
      refuseIfPresent: ledgerTable("fleet-sync-receiver"),
      statements,
    });
  }
  revertFleet() {
    return this.call<RevertReply>(this.namespaces.FLEET, "fleet", OBJECT_REVERT_URL, {
      requirePresent: ledgerTable("central-fleet"),
      // Children before parents, so no foreign key is left dangling mid-drop.
      drop: [
        ...(["central-fleet", "central-registry", "fleet-sync-lease"] as const).map(ledgerTable),
        ...[...MAIN_FLEET_CREATION_ORDER].reverse(),
      ],
      renameBack: FLEET_RENAMED.map((table) => [legacyName(table), table]),
      journal: JOURNAL_REVERT,
    });
  }
  rehearseFleet(statements: readonly Statement[]) {
    return this.call<RehearseReply>(this.namespaces.FLEET, "fleet", OBJECT_REHEARSE_URL, {
      statements,
      exclude: [],
    }).then((reply) => reply.image);
  }
  rehearseCopy(project: string, statements: readonly Statement[], exclude: readonly string[]) {
    return this.call<RehearseReply>(this.namespaces.PROJECT_COPIES, project, OBJECT_REHEARSE_URL, {
      statements,
      exclude,
    }).then((reply) => reply.image);
  }
  revertCopy(project: string) {
    return this.call<RevertReply>(this.namespaces.PROJECT_COPIES, project, OBJECT_REVERT_URL, {
      requirePresent: ledgerTable("fleet-sync-receiver"),
      drop: [ledgerTable("fleet-sync-receiver"), ...COPY_RENAMED],
      renameBack: COPY_RENAMED.map((table) => [legacyName(table), table]),
      journal: JOURNAL_REVERT,
    });
  }
}

/** What the calls need besides the objects: the run's clock and central's configuration. */
export interface CallContext {
  /** The run's one instant in milliseconds; the calls round it to whole seconds. */
  readonly now: () => number;
  /** Old central's CENTRAL_REGISTRY_CONFIG text, which names its authoritative receiver. */
  readonly registryConfig: string | undefined;
}

export interface CallAnswer {
  readonly status: number;
  readonly body: Record<string, unknown>;
}

/** Which objects already hold main's records: each object's own migrated marker. */
interface MigratedObjects {
  readonly fleet: boolean;
  readonly copies: Readonly<Record<string, boolean>>;
}

async function migratedObjects(objects: StorageObjects): Promise<MigratedObjects> {
  const fleet = (await objects.fleet([])).tables.includes(ledgerTable("central-fleet"));
  const copies: Record<string, boolean> = {};
  for (const project of await objects.projects()) {
    copies[project] = (await objects.copy(project, [])).tables.includes(
      ledgerTable("fleet-sync-receiver"),
    );
  }
  return { fleet, copies };
}

async function readOld(objects: StorageObjects): Promise<OldStorage> {
  const registry = await objects.registry(REGISTRY_READ);
  const fleet = await objects.fleet(FLEET_READ);
  const copies: Record<string, ReadReply> = {};
  for (const project of await objects.projects())
    copies[project] = await objects.copy(project, COPY_READ);
  return { registry, fleet, copies };
}

const wholeSeconds = (millis: number): number => Math.floor(millis / 1000) * 1000;

const report = (plan: MigrationPlan) => ({
  runTime: new Date(plan.runTime).toISOString(),
  configuredReceiver: plan.configuredReceiver,
  stops: plan.stops,
  projects: plan.projects,
  counts: plan.counts,
});

/**
 * The three retiring classes in full: after PD7 step 5 deletes them, this dump
 * is the only copy of their storage. A Connection and a Mailbox are named by
 * the Router installation (O object_location.rs:6,8), from the enrollments.
 */
async function retiringDump(objects: StorageObjects) {
  const registry = await objects.dump(objects.namespaces.LEASE_REGISTRY, "registry");
  const services = [
    ...new Set((registry.rows.machine_enrollments ?? []).map((row) => String(row.service_id))),
  ].sort();
  const connections: Record<string, ObjectDump> = {};
  const mailboxes: Record<string, ObjectDump> = {};
  for (const service of services) {
    connections[service] = await objects.dump(objects.namespaces.CONNECTIONS, service);
    mailboxes[service] = await objects.dump(objects.namespaces.MAILBOXES, service);
  }
  return {
    registry,
    connections,
    mailboxes,
    // Durable Objects can't be listed, so only enrolled services' objects are
    // found; a service no longer enrolled is missed (mail and connection
    // state aren't carried anyway, R13).
    coverage: "Connection and Mailbox objects of currently enrolled services only",
  };
}

const everyMigrated = (state: MigratedObjects): boolean =>
  state.fleet && Object.values(state.copies).every(Boolean);
const anyMigrated = (state: MigratedObjects): boolean =>
  state.fleet || Object.values(state.copies).some(Boolean);

/** Read-only. With S2's facts it makes migrate's whole decision; without, it reports central's side. */
export async function inventory(
  objects: StorageObjects,
  context: CallContext,
  macFiles: readonly MacFileEvidence[] | null,
): Promise<CallAnswer> {
  const retiring = await retiringDump(objects);
  if ((await migratedObjects(objects)).fleet) {
    return {
      status: 200,
      body: { kind: "alreadyMigrated", retiring, rowsWritten: objects.rowsWritten },
    };
  }
  const old = await readOld(objects);
  const runTime = wholeSeconds(context.now());
  const plan = await planMigration(old, {
    runTime,
    registryConfig: context.registryConfig,
    macFiles,
  });
  // With S2's facts and no stop, the candidate the Rust gate decodes and migrate must reproduce.
  const candidate =
    macFiles !== null && plan.stops.length === 0
      ? await rehearseCandidate(
          objects as CandidateSource,
          plan,
          old,
          await migratedObjects(objects),
        )
      : null;
  return {
    status: 200,
    body: {
      kind: "inventory",
      ...report(plan),
      runTimeMillis: runTime,
      ...(candidate === null
        ? {}
        : { candidateDigest: candidate.digest, candidateText: candidate.text }),
      retiring,
      rowsWritten: objects.rowsWritten,
    },
  };
}

/**
 * Copies first, the Fleet last: a rerun after an interruption finds the Fleet
 * unmigrated, replans from the untouched old sources, and applies only to the
 * objects still old (each migrated object refuses its write itself).
 */
export async function migrate(
  objects: StorageObjects,
  context: CallContext,
  request: MigrateRequest,
): Promise<CallAnswer> {
  const state = await migratedObjects(objects);
  if (everyMigrated(state)) {
    return { status: 200, body: { kind: "alreadyMigrated", rowsWritten: objects.rowsWritten } };
  }
  if (state.fleet) {
    // Only an unfinished revert leaves the Fleet migrated beside an old copy.
    const reverted = Object.entries(state.copies).filter(([, migrated]) => !migrated);
    return {
      status: 409,
      body: {
        kind: "stopped",
        stops: reverted.map(([project]) => ({
          object: `copy ${project}`,
          subject: `project ${project}`,
          reason:
            "this copy is reverted while the Fleet is still migrated; run revert to completion first",
        })),
        rowsWritten: objects.rowsWritten,
      },
    };
  }
  const old = await readOld(objects);
  const plan = await planMigration(old, {
    runTime: request.passed?.runTime ?? wholeSeconds(context.now()),
    registryConfig: context.registryConfig,
    macFiles: request.files,
  });
  if (plan.stops.length > 0) {
    return {
      status: 409,
      body: { kind: "stopped", ...report(plan), rowsWritten: objects.rowsWritten },
    };
  }
  // Only the candidate the Rust gate passed may be written: rebuild it at the
  // gate's run time and refuse, before any write, unless it is byte-identical.
  const refusal = (reason: string): CallAnswer => ({
    status: 409,
    body: {
      kind: "stopped",
      ...report(plan),
      stops: [{ object: "candidate", subject: "decode gate", reason }],
      rowsWritten: objects.rowsWritten,
    },
  });
  if (request.passed === null) {
    return refusal("migrate needs the run time and candidate digest the Rust decode gate passed");
  }
  const candidate = await rehearseCandidate(objects as CandidateSource, plan, old, state);
  if (candidate.digest !== request.passed.candidateDigest) {
    return refusal(
      `the candidate is ${candidate.digest}, not the ${request.passed.candidateDigest} the gate passed`,
    );
  }
  const copies: Record<string, ApplyReply["kind"]> = {};
  for (const [project, statements] of Object.entries(plan.copies)) {
    copies[project] = (await objects.applyCopy(project, statements)).kind;
  }
  const fleet = await objects.applyFleet(plan.fleet);
  return {
    status: 200,
    body: {
      kind: "migrated",
      ...report(plan),
      fleet: fleet.kind,
      copies,
      rowsWritten: objects.rowsWritten,
    },
  };
}

export async function verify(objects: StorageObjects): Promise<CallAnswer> {
  const registry = await objects.registry(REGISTRY_READ);
  const fleet = await objects.fleet(FLEET_VERIFY_READ);
  const copies: Record<string, ReadReply> = {};
  for (const project of await objects.projects())
    copies[project] = await objects.copy(project, COPY_VERIFY_READ);
  const result = await verifyMigration(registry, fleet, copies);
  return {
    status: result.ok ? 200 : 409,
    body: { kind: "verified", ...result, rowsWritten: objects.rowsWritten },
  };
}

/**
 * The reverse order of migrate: every copy, then the Fleet, whose migrated
 * marker is the last to go. Each object reverts in one transaction and skips
 * itself when already old, so a revert interrupted anywhere, or run after a
 * migrate that stopped after some copies, finishes by running it again.
 */
export async function revert(objects: StorageObjects): Promise<CallAnswer> {
  const state = await migratedObjects(objects);
  if (!anyMigrated(state)) {
    return { status: 409, body: { kind: "notMigrated", rowsWritten: objects.rowsWritten } };
  }
  const copies: Record<string, RevertReply["kind"]> = {};
  for (const project of Object.keys(state.copies)) {
    copies[project] = (await objects.revertCopy(project)).kind;
  }
  const fleet = await objects.revertFleet();
  return {
    status: 200,
    body: { kind: "reverted", fleet: fleet.kind, copies, rowsWritten: objects.rowsWritten },
  };
}
