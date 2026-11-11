// The one-time central migration Worker (design PD2). It is deployed under
// central's own name with the same five storage classes, and exposes exactly
// four owner-authenticated admin calls: inventory (read-only), migrate, verify
// and revert. Every other request, including Routers' /v1/session, is refused
// while it is deployed. Deleted after production (R11).
// S1 alone also exposes a read-only storage snapshot for staging verification.

import { parseExact, stringifyExact } from "./exact_json.ts";
import { parseMacFiles, parseMigrateRequest } from "./mac_file_evidence.ts";
import {
  StorageObjects,
  inventory,
  migrate,
  revert,
  verify,
  type CallAnswer,
  type CallContext,
  type StorageNamespaces,
} from "./migration_calls.ts";
import { accessPolicy, checkOwner, type AccessSettings } from "./owner_access.ts";
import { Interrupted, storageObjectsFor, type FaultEnv } from "./fault_plan.ts";
import {
  STAGING_SNAPSHOT_PATH,
  STAGING_SNAPSHOT_WORKER,
  stagingSnapshot,
} from "./staging_snapshot.ts";

export {
  ConnectionObject,
  FleetObject,
  LeaseRegistryObject,
  MailboxObject,
  ProjectCopyObject,
} from "./storage_objects.ts";

/** Deployed with the live central's own bindings (PD7 step 2), its registry configuration included. */
export interface MigrationEnv extends AccessSettings, StorageNamespaces, FaultEnv {
  readonly CENTRAL_REGISTRY_CONFIG?: string;
}

/**
 * Inventory is read-only either way: GET reports central's side, POST with
 * migrate's own body makes migrate's whole decision as a dry run.
 */
const ROUTES: Record<string, { readonly methods: readonly string[]; readonly call: string }> = {
  "/admin/v1/migration/inventory": { methods: ["GET", "POST"], call: "inventory" },
  "/admin/v1/migration/migrate": { methods: ["POST"], call: "migrate" },
  "/admin/v1/migration/verify": { methods: ["GET"], call: "verify" },
  "/admin/v1/migration/revert": { methods: ["POST"], call: "revert" },
};

const json = (value: unknown, status = 200): Response =>
  new Response(stringifyExact(value), {
    status,
    headers: { "content-type": "application/json" },
  });
const answer = ({ status, body }: CallAnswer): Response => json(body, status);

/** Inventory's body: the S2 facts read from each holder Mac's retained project file. */
async function macFilesOf(request: Request) {
  try {
    return parseMacFiles(parseExact(await request.text()));
  } catch (error) {
    return { refused: error instanceof Error ? error.message : String(error) };
  }
}

/** Migrate's body: the same facts, plus the run time and candidate digest the decode gate passed. */
async function migrateRequestOf(request: Request) {
  try {
    return { request: parseMigrateRequest(parseExact(await request.text())) };
  } catch (error) {
    return { refused: error instanceof Error ? error.message : String(error) };
  }
}

export default {
  async fetch(request: Request, env: MigrationEnv): Promise<Response> {
    const path = new URL(request.url).pathname;
    const route =
      path === STAGING_SNAPSHOT_PATH && env.MIGRATION_WORKER_NAME === STAGING_SNAPSHOT_WORKER
        ? { methods: ["POST"], call: "staging-snapshot" }
        : ROUTES[path];
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
    if (!route.methods.includes(request.method)) return json({ refused: "method" }, 405);
    // The S1 staging interruption hook applies only to a deployment naming itself.
    const objects = storageObjectsFor(env);
    const context: CallContext = {
      now: () => Date.now(),
      registryConfig: env.CENTRAL_REGISTRY_CONFIG,
    };
    try {
      return await call(route.call, request, objects, context);
    } catch (error) {
      if (error instanceof Interrupted)
        return json({ interrupted: error.message, committed: error.committed }, 500);
      throw error;
    }
  },
};

async function call(
  name: string,
  request: Request,
  objects: StorageObjects,
  context: CallContext,
): Promise<Response> {
  switch (name) {
    case "staging-snapshot": {
      let body: unknown;
      try {
        body = parseExact(await request.text());
      } catch {
        return json({ refused: "body", reason: "not JSON" }, 400);
      }
      return answer(await stagingSnapshot(body, objects));
    }
    case "inventory": {
      if (request.method === "GET") return answer(await inventory(objects, context, null));
      const macFiles = await macFilesOf(request);
      if ("refused" in macFiles)
        return json({ refused: "macFiles", reason: macFiles.refused }, 400);
      return answer(await inventory(objects, context, macFiles.files));
    }
    case "migrate": {
      const parsed = await migrateRequestOf(request);
      if ("refused" in parsed) return json({ refused: "body", reason: parsed.refused }, 400);
      return answer(await migrate(objects, context, parsed.request));
    }
    case "verify":
      return answer(await verify(objects, context));
    default:
      return answer(await revert(objects));
  }
}
