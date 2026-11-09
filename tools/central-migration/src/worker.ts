// The one-time central migration Worker (design PD2). It is deployed under
// central's own name with the same five storage classes, and exposes exactly
// four owner-authenticated admin calls: inventory (read-only), migrate, verify
// and revert. Every other request, including Routers' /v1/session, is refused
// while it is deployed. Deleted after production (R11).

import { COPY_READ, FLEET_READ, REGISTRY_READ, planMigration } from "./migration_plan.ts";
import type { MigrationPlan, OldStorage } from "./migration_plan.ts";
import { COPY_VERIFY_READ, FLEET_VERIFY_READ, verifyMigration } from "./migration_verify.ts";
import { accessPolicy, checkOwner, type AccessSettings } from "./owner_access.ts";
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
  OBJECT_REVERT_URL,
  type ApplyReply,
  type ObjectDump,
  type ReadReply,
  type RevertReply,
  type Statement,
} from "./storage_objects.ts";

export {
  ConnectionObject,
  FleetObject,
  LeaseRegistryObject,
  MailboxObject,
  ProjectCopyObject,
} from "./storage_objects.ts";

export interface MigrationEnv extends AccessSettings {
  readonly LEASE_REGISTRY: DurableObjectNamespace;
  readonly CONNECTIONS: DurableObjectNamespace;
  readonly PROJECT_COPIES: DurableObjectNamespace;
  readonly FLEET: DurableObjectNamespace;
  readonly MAILBOXES: DurableObjectNamespace;
}

const ROUTES: Record<string, { readonly method: string; readonly call: string }> = {
  "/admin/v1/migration/inventory": { method: "GET", call: "inventory" },
  "/admin/v1/migration/migrate": { method: "POST", call: "migrate" },
  "/admin/v1/migration/verify": { method: "GET", call: "verify" },
  "/admin/v1/migration/revert": { method: "POST", call: "revert" },
};

const json = (value: unknown, status = 200): Response =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

/**
 * Object names follow production's locations
 * (O crates/collaboration-central-cloudflare/src/object_location.rs:3–9).
 */
class Objects {
  readonly env: MigrationEnv;
  rowsWritten = 0;
  constructor(env: MigrationEnv) {
    this.env = env;
  }
  private async call<T>(
    namespace: DurableObjectNamespace,
    name: string,
    url: string,
    body?: unknown,
  ): Promise<T> {
    const response = await namespace
      .get(namespace.idFromName(name))
      .fetch(url, body === undefined ? undefined : { method: "POST", body: JSON.stringify(body) });
    if (!response.ok)
      throw new Error(`${name}: ${url} answered ${response.status}: ${await response.text()}`);
    const reply = (await response.json()) as T & { rowsWritten?: number };
    this.rowsWritten += reply.rowsWritten ?? 0;
    return reply;
  }
  registry = (tables: readonly string[]) =>
    this.call<ReadReply>(this.env.LEASE_REGISTRY, "registry", OBJECT_READ_URL, { tables });
  fleet = (tables: readonly string[]) =>
    this.call<ReadReply>(this.env.FLEET, "fleet", OBJECT_READ_URL, { tables });
  copy = (project: string, tables: readonly string[]) =>
    this.call<ReadReply>(this.env.PROJECT_COPIES, project, OBJECT_READ_URL, { tables });
  dump = (namespace: DurableObjectNamespace, name: string) =>
    this.call<ObjectDump>(namespace, name, OBJECT_DUMP_URL);
  projects = () =>
    this.call<string[]>(this.env.LEASE_REGISTRY, "registry", OBJECT_REGISTRY_PROJECTS_URL);
  applyFleet = (statements: readonly Statement[]) =>
    this.call<ApplyReply>(this.env.FLEET, "fleet", OBJECT_APPLY_URL, {
      refuseIfPresent: ledgerTable("central-fleet"),
      statements,
    });
  applyCopy = (project: string, statements: readonly Statement[]) =>
    this.call<ApplyReply>(this.env.PROJECT_COPIES, project, OBJECT_APPLY_URL, {
      refuseIfPresent: ledgerTable("fleet-sync-receiver"),
      statements,
    });
  revertFleet = () =>
    this.call<RevertReply>(this.env.FLEET, "fleet", OBJECT_REVERT_URL, {
      requirePresent: ledgerTable("central-fleet"),
      // Children before parents, so no foreign key is left dangling mid-drop.
      drop: [
        ...(["central-fleet", "central-registry", "fleet-sync-lease"] as const).map(ledgerTable),
        ...[...MAIN_FLEET_CREATION_ORDER].reverse(),
      ],
      renameBack: FLEET_RENAMED.map((table) => [legacyName(table), table]),
    });
  revertCopy = (project: string) =>
    this.call<RevertReply>(this.env.PROJECT_COPIES, project, OBJECT_REVERT_URL, {
      requirePresent: ledgerTable("fleet-sync-receiver"),
      drop: [ledgerTable("fleet-sync-receiver"), ...COPY_RENAMED],
      renameBack: COPY_RENAMED.map((table) => [legacyName(table), table]),
    });
}

async function readOld(objects: Objects): Promise<OldStorage> {
  const registry = await objects.registry(REGISTRY_READ);
  const fleet = await objects.fleet(FLEET_READ);
  const copies: Record<string, ReadReply> = {};
  for (const project of await objects.projects())
    copies[project] = await objects.copy(project, COPY_READ);
  return { registry, fleet, copies };
}

/** One instant per run, whole seconds, so installed_on and created_at agree. */
const runTimeNow = (): number => Math.floor(Date.now() / 1000) * 1000;

const report = (plan: MigrationPlan) => ({
  runTime: new Date(plan.runTime).toISOString(),
  stops: plan.stops,
  projects: plan.projects,
  counts: plan.counts,
});

async function migrated(objects: Objects): Promise<boolean> {
  return (await objects.fleet([])).tables.includes(ledgerTable("central-fleet"));
}

/**
 * The three retiring classes in full: after PD7 step 5 deletes them, this dump
 * is the only copy of their storage. A Connection and a Mailbox are named by
 * the Router installation (O object_location.rs:6,8), from the enrollments.
 */
async function retiringDump(objects: Objects) {
  const registry = await objects.dump(objects.env.LEASE_REGISTRY, "registry");
  const services = [
    ...new Set((registry.rows.machine_enrollments ?? []).map((row) => String(row.service_id))),
  ].sort();
  const connections: Record<string, ObjectDump> = {};
  const mailboxes: Record<string, ObjectDump> = {};
  for (const service of services) {
    connections[service] = await objects.dump(objects.env.CONNECTIONS, service);
    mailboxes[service] = await objects.dump(objects.env.MAILBOXES, service);
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

async function inventory(objects: Objects) {
  const retiring = await retiringDump(objects);
  if (await migrated(objects)) {
    return json({ kind: "alreadyMigrated", retiring, rowsWritten: objects.rowsWritten });
  }
  const plan = await planMigration(await readOld(objects), runTimeNow());
  return json({ kind: "inventory", ...report(plan), retiring, rowsWritten: objects.rowsWritten });
}

async function migrate(objects: Objects) {
  if (await migrated(objects))
    return json({ kind: "alreadyMigrated", rowsWritten: objects.rowsWritten });
  const plan = await planMigration(await readOld(objects), runTimeNow());
  if (plan.stops.length > 0) {
    return json({ kind: "stopped", ...report(plan), rowsWritten: objects.rowsWritten }, 409);
  }
  // Copies first, the Fleet object last: a rerun after an interruption finds
  // the Fleet unmigrated and finishes, and each migrated copy refuses itself.
  const copies: Record<string, ApplyReply["kind"]> = {};
  for (const [project, statements] of Object.entries(plan.copies)) {
    copies[project] = (await objects.applyCopy(project, statements)).kind;
  }
  const fleet = await objects.applyFleet(plan.fleet);
  return json({
    kind: "migrated",
    ...report(plan),
    fleet: fleet.kind,
    copies,
    rowsWritten: objects.rowsWritten,
  });
}

async function verify(objects: Objects) {
  const registry = await objects.registry(REGISTRY_READ);
  const fleet = await objects.fleet(FLEET_VERIFY_READ);
  const copies: Record<string, ReadReply> = {};
  for (const project of await objects.projects())
    copies[project] = await objects.copy(project, COPY_VERIFY_READ);
  const result = await verifyMigration(registry, fleet, copies);
  return json(
    { kind: "verified", ...result, rowsWritten: objects.rowsWritten },
    result.ok ? 200 : 409,
  );
}

async function revert(objects: Objects) {
  if (!(await migrated(objects)))
    return json({ kind: "notMigrated", rowsWritten: objects.rowsWritten }, 409);
  const fleet = await objects.revertFleet();
  const copies: Record<string, RevertReply["kind"]> = {};
  for (const project of await objects.projects())
    copies[project] = (await objects.revertCopy(project)).kind;
  return json({ kind: "reverted", fleet: fleet.kind, copies, rowsWritten: objects.rowsWritten });
}

export default {
  async fetch(request: Request, env: MigrationEnv): Promise<Response> {
    const route = ROUTES[new URL(request.url).pathname];
    if (route === undefined) return json({ refused: "central is being migrated" }, 503);
    let policy;
    try {
      policy = accessPolicy(env);
    } catch {
      return json({ refused: "invalidCaller" }, 403);
    }
    const owner = await checkOwner(
      request.headers.get("Cf-Access-Jwt-Assertion"),
      policy,
      Math.floor(Date.now() / 1000),
      async (certsUrl) => {
        const response = await fetch(certsUrl);
        if (!response.ok) throw new Error(`certs answered ${response.status}`);
        return response.json();
      },
    );
    if (owner.kind === "refused") return json({ refused: owner.refusal }, 403);
    if (request.method !== route.method) return json({ refused: "method" }, 405);
    const objects = new Objects(env);
    switch (route.call) {
      case "inventory":
        return inventory(objects);
      case "migrate":
        return migrate(objects);
      case "verify":
        return verify(objects);
      default:
        return revert(objects);
    }
  },
};
