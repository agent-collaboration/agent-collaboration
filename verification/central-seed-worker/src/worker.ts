// Staging-only seed Worker (staging-seed-worker-spec.md). Deployed under
// staging central's name with the five storage classes, it writes old-shape
// Registry, Fleet and Copy storage derived from authorized project copies, so
// the exact old central (2636806a5) can then be redeployed over it. It refuses
// unless its STAGING_SEED_TARGET binding is set and the request names the same
// target, writes only into fresh objects, and never seeds enrollments,
// receipts or requests. Removed with the migration tooling after production.

import {
  accessPolicy,
  checkOwner,
  type AccessSettings,
} from "../../../tools/central-migration/src/owner_access.ts";
import type { DurableObjectNamespace } from "./platform.ts";
import {
  SeedRefused,
  checkProject,
  unsigned,
  uuidV7,
  type CopiesRequest,
  type RegistryFacts,
  type RegistryRequest,
  type SeedProject,
} from "./seed_contract.ts";
import { copyPreludeStatements, fleetStatements, registryStatements } from "./seed_storage.ts";
import {
  OBJECT_COPY_RECORD_URL,
  OBJECT_COPY_URL,
  OBJECT_STATEMENTS_URL,
  type ObjectReply,
} from "./storage_objects.ts";
import { tableDigest } from "./table_digest.ts";
import { transportToSeed } from "./seed_contract.ts";

export {
  ConnectionObject,
  FleetObject,
  LeaseRegistryObject,
  MailboxObject,
  ProjectCopyObject,
} from "./storage_objects.ts";

import { isS4Route, s4Route } from "./s4_routes.ts";

export interface SeedEnv extends AccessSettings {
  /** Set only by the staging deploy config; any other deployment refuses every call. */
  readonly STAGING_SEED_TARGET?: string;
  readonly LEASE_REGISTRY: DurableObjectNamespace;
  readonly CONNECTIONS: DurableObjectNamespace;
  readonly PROJECT_COPIES: DurableObjectNamespace;
  readonly FLEET: DurableObjectNamespace;
  readonly MAILBOXES: DurableObjectNamespace;
}

const ROUTES: Record<string, "copies" | "registry"> = {
  "/admin/v1/staging-seed/copies": "copies",
  "/admin/v1/staging-seed/registry": "registry",
};

const json = (value: unknown, status = 200): Response =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

function uuidV7Now(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  let time = BigInt(Date.now());
  for (let index = 5; index >= 0; index -= 1) {
    bytes[index] = Number(time & 0xffn);
    time >>= 8n;
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

async function post(
  namespace: DurableObjectNamespace,
  name: string,
  url: string,
  body: unknown,
): Promise<ObjectReply> {
  const response = await namespace
    .get(namespace.idFromName(name))
    .fetch(url, { method: "POST", body: JSON.stringify(body) });
  if (!response.ok)
    throw new Error(`${name}: ${url} answered ${response.status}: ${await response.text()}`);
  return (await response.json()) as ObjectReply;
}

function checkTarget(target: unknown, env: SeedEnv): void {
  if (env.STAGING_SEED_TARGET === undefined || env.STAGING_SEED_TARGET === "") {
    throw new SeedRefused("this deployment is not a staging seed target");
  }
  if (target !== env.STAGING_SEED_TARGET) {
    throw new SeedRefused(
      `request target ${String(target)} is not this deployment's ${env.STAGING_SEED_TARGET}`,
    );
  }
}

function distinct<T extends { readonly database: string }>(projects: readonly T[]): readonly T[] {
  if (!(projects instanceof Array) || projects.length === 0) throw new SeedRefused("no projects");
  const seen = new Set<string>();
  for (const project of projects) {
    if (seen.has(uuidV7(project.database, "database")))
      throw new SeedRefused(`project ${project.database} twice`);
    seen.add(project.database);
  }
  return projects;
}

/** Phase 1: checks every project, including each table's rows against its manifest digest, before any object is touched. */
async function seedCopies(request: CopiesRequest, env: SeedEnv) {
  checkTarget(request.target, env);
  const projects: readonly SeedProject[] = distinct(request.projects);
  for (const project of projects) {
    checkProject(project);
    for (const entry of project.manifest) {
      const rows = project.pages
        .filter((page) => page.table === entry.table)
        .flatMap((page) => page.rows)
        .map((row, index) =>
          row.map((value, column) => transportToSeed(value, `${entry.table}[${index}][${column}]`)),
        );
      if (tableDigest(rows) !== entry.digest) {
        throw new SeedRefused(
          `project ${project.database}: input rows of ${entry.table} don't match their manifest digest`,
        );
      }
    }
  }
  const results: Record<string, ObjectReply> = {};
  for (const project of projects) {
    results[`copy ${project.database}`] = await post(
      env.PROJECT_COPIES,
      project.database,
      OBJECT_COPY_URL,
      {
        prelude: await copyPreludeStatements(uuidV7Now),
        project,
      },
    );
  }
  const refused = Object.values(results).some((reply) => reply.kind === "refused");
  return { kind: refused ? ("refused" as const) : ("seeded" as const), results };
}

/** Phase 2: only after every listed Copy is seeded Following with the same holder and lease number. */
async function seedRegistry(request: RegistryRequest, env: SeedEnv) {
  checkTarget(request.target, env);
  unsigned(request.registryIncarnation, "registryIncarnation", true);
  const projects: readonly RegistryFacts[] = distinct(request.projects);
  for (const project of projects) {
    uuidV7(project.holder, `project ${project.database}.holder`);
    unsigned(project.leaseNumber, `project ${project.database}.leaseNumber`, true);
    unsigned(project.seededAt, `project ${project.database}.seededAt`);
    unsigned(project.expiresAt, `project ${project.database}.expiresAt`);
    const stub = env.PROJECT_COPIES.get(env.PROJECT_COPIES.idFromName(project.database));
    const record = (await (await stub.fetch(OBJECT_COPY_RECORD_URL)).json()) as string | null;
    const expected = `"fence":${project.leaseNumber},"holder":${JSON.stringify(project.holder)},`;
    if (
      record === null ||
      !record.startsWith('{"state":"following"') ||
      !record.includes(expected)
    ) {
      throw new SeedRefused(
        `project ${project.database}: its Copy isn't seeded Following at holder ${project.holder}, lease ${project.leaseNumber}`,
      );
    }
  }
  const results: Record<string, ObjectReply> = {
    registry: await post(
      env.LEASE_REGISTRY,
      "registry",
      OBJECT_STATEMENTS_URL,
      await registryStatements(request, uuidV7Now),
    ),
  };
  results.fleet = await post(
    env.FLEET,
    "fleet",
    OBJECT_STATEMENTS_URL,
    await fleetStatements(request, uuidV7Now),
  );
  const refused = Object.values(results).some((reply) => reply.kind === "refused");
  return { kind: refused ? ("refused" as const) : ("seeded" as const), results };
}

export default {
  async fetch(request: Request, env: SeedEnv): Promise<Response> {
    const path = new URL(request.url).pathname;
    const phase = ROUTES[path];
    if (phase === undefined && !isS4Route(path))
      return json({ refused: "staging seed Worker" }, 503);
    if (env.STAGING_SEED_TARGET === undefined || env.STAGING_SEED_TARGET === "") {
      return json({ refused: "this deployment is not a staging seed target" }, 503);
    }
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
    if (request.method !== "POST") return json({ refused: "method" }, 405);
    if (phase === undefined) return s4Route(path, await request.json(), env);
    try {
      const body: unknown = await request.json();
      const result =
        phase === "copies"
          ? await seedCopies(body as CopiesRequest, env)
          : await seedRegistry(body as RegistryRequest, env);
      return json(result, result.kind === "seeded" ? 200 : 409);
    } catch (error) {
      if (error instanceof SeedRefused)
        return json({ kind: "refused", reason: error.message }, 409);
      throw error;
    }
  },
};
