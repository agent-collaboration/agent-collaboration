// Test only: the exact old central (2636806a5), built with worker-build, run in
// real workerd over a given persistence path with its public template's text
// bindings filled for a test. Needs OLD_CENTRAL_BUILD.

import { join } from "node:path";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";
import { ACCESS, CERTS_URL } from "./workerd_harness.ts";

/** The authoritative receiver old central is configured with in these tests (staging C). */
export const RECEIVER = "01a16eaa-ae36-7eec-9698-e8c34d10238b";

export function oldCentralBuild(): string {
  const build = process.env.OLD_CENTRAL_BUILD;
  if (build === undefined || build === "")
    throw new Error("OLD_CENTRAL_BUILD must name the 2636806a5 Worker build");
  return build;
}

export async function startOldCentral(
  build: string,
  persistence: string,
  jwks: unknown,
): Promise<Miniflare> {
  const template = (await import(join(build, "../cloudflare.config.ts"))) as {
    default: { worker: { env: Record<string, { type: string; value?: string }> } };
  };
  const replacements: Record<string, string> = {
    ...ACCESS,
    CENTRAL_REGISTRY_CONFIG: JSON.stringify({
      lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
      authoritativeReceiver: RECEIVER,
      releaseUnsentFor: { secs: 0, nanos: 0 },
      moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
    }),
    CENTRAL_DELIVERY_CONFIG: JSON.stringify({ relayPeer: "01a1c0c5-0000-7000-8000-00000000c001" }),
  };
  const bindings = Object.fromEntries(
    Object.entries(template.default.worker.env)
      .filter(([, binding]) => binding.type === "text")
      .map(([name, binding]) => [name, replacements[name] ?? binding.value ?? ""]),
  );
  const runtime = new Miniflare(
    convertV4MiniflareOptions({
      resourcePersistencePath: persistence,
      cf: false,
      workers: [
        {
          name: "collaboration-central",
          compatibilityDate: "2026-08-21",
          modulesRoot: build,
          modules: [
            { type: "ESModule", path: join(build, "index.js") },
            { type: "CompiledWasm", path: join(build, "index_bg.wasm") },
          ],
          durableObjects: {
            LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
            CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
            PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
            FLEET: { className: "FleetObject", useSQLite: true },
            MAILBOXES: { className: "MailboxObject", useSQLite: true },
          },
          bindings,
          outboundService: async (request: Request) => {
            if (request.url !== CERTS_URL)
              throw new Error(`unexpected outbound fetch: ${request.url}`);
            return MiniflareResponse.json(jwks);
          },
        },
      ],
    }),
  );
  await runtime.ready;
  return runtime;
}
