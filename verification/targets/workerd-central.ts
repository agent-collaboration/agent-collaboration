import { mkdir, readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { Miniflare, Response, convertV4MiniflareOptions } from "miniflare";
import { z } from "zod";
import { CentralOwner } from "../clients/central-owner.ts";
import { fingerprint, privateDirectory } from "../machines/owned-process.ts";
import type { Target } from "../declaration.ts";
import type { CentralTarget } from "./central-target.ts";

export class WorkerdCentral implements CentralTarget {
  readonly origin: string;
  readonly owner: CentralOwner;
  readonly runtime: Miniflare;
  private constructor(origin: string, owner: CentralOwner, runtime: Miniflare) {
    this.origin = origin;
    this.owner = owner;
    this.runtime = runtime;
  }
  static async start(target: Extract<Target, { kind: "workerd" }>): Promise<WorkerdCentral> {
    const require = createRequire(import.meta.url);
    z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
    z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
    if (
      (await fingerprint(`${target.bundleDirectory}/index_bg.wasm`)) !== target.wasmSHA256 ||
      (await fingerprint(`${target.bundleDirectory}/index.js`)) !== target.javascriptSHA256
    )
      throw new Error("Worker bundle mismatch");
    await mkdir(target.root, { mode: 0o700 });
    await privateDirectory(target.root);
    const source: unknown = await import(pathToFileURL(target.template).href);
    const template = z
      .object({
        default: z.object({
          worker: z.object({
            compatibilityDate: z.string(),
            env: z.record(
              z.string(),
              z.discriminatedUnion("type", [
                z.object({ type: z.literal("text"), value: z.string() }),
                z.object({ type: z.literal("durable-object"), exportName: z.string() }),
              ]),
            ),
          }),
        }),
      })
      .parse(source).default.worker;
    const replacements: Readonly<Record<string, string>> = {
      ACCESS_TEAM_DOMAIN: "conformance-test.cloudflareaccess.com",
      ACCESS_SESSION_AUD: "test-session-aud",
      ACCESS_ADMIN_AUD: "test-admin-aud",
      ACCESS_OWNER_IDENTITY: "owner@example.test",
      CENTRAL_HOLDER_ANSWER_TIMEOUT: JSON.stringify({ secs: 30, nanos: 0 }),
      CENTRAL_COPY_ACK_WAIT: JSON.stringify({ secs: 20, nanos: 0 }),
      CENTRAL_REGISTRY_CONFIG: JSON.stringify({
        lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
        authoritativeReceiver: "019f0000-0000-7000-8000-000000000010",
        releaseUnsentFor: { secs: 1800, nanos: 0 },
        moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
      }),
      CENTRAL_DELIVERY_CONFIG: JSON.stringify({
        relayPeer: "019f0000-0000-7000-8000-000000000011",
      }),
    };
    const bindings: Record<string, string> = {};
    const objects: Record<string, { className: string; useSQLite: true }> = {};
    for (const [name, binding] of Object.entries(template.env))
      if (binding.type === "text") bindings[name] = replacements[name] ?? binding.value;
      else objects[name] = { className: binding.exportName, useSQLite: true };
    const jwks = await readFile(target.jwksFile, "utf8");
    const runtime = new Miniflare(
      convertV4MiniflareOptions({
        host: "127.0.0.1",
        port: 0,
        resourcePersistencePath: target.root,
        cf: false,
        workers: [
          {
            name: "verification-central",
            compatibilityDate: template.compatibilityDate,
            modulesRoot: target.bundleDirectory,
            modules: [
              { type: "ESModule", path: `${target.bundleDirectory}/index.js` },
              { type: "CompiledWasm", path: `${target.bundleDirectory}/index_bg.wasm` },
            ],
            bindings,
            durableObjects: objects,
            cacheAPI: true,
            outboundService: async (request): Promise<Response> => {
              if (
                request.url !== "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs"
              )
                throw new Error("unexpected external workerd fetch");
              return new Response(jwks, { headers: { "content-type": "application/json" } });
            },
          },
        ],
      }),
    );
    try {
      const ready = await runtime.ready;
      if (ready.hostname !== "127.0.0.1" || [8787, 8788, 43127].includes(Number(ready.port)))
        throw new Error("workerd did not bind permitted loopback");
      const assertion = (await readFile(target.assertionFile, "utf8")).trim();
      return new WorkerdCentral(
        ready.origin,
        new CentralOwner(ready.origin, { "Cf-Access-Jwt-Assertion": assertion }),
        runtime,
      );
    } catch (error: unknown) {
      await runtime.dispose();
      throw error;
    }
  }
  async close(): Promise<void> {
    await this.runtime.dispose();
  }
}
