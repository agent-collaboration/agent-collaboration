// Local harness only: the compiled Rust Worker and workerd APIs remain real.
// Redirect exactly the fixture issuer's certs fetch to the owned HTTP server.
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const [miniflareEntrypoint, persistence, portText, jwksUrl] = process.argv.slice(2);
if (!miniflareEntrypoint || !persistence || !portText || !jwksUrl) {
  throw new Error("Miniflare entrypoint, isolated persistence, port and fixture JWKS URL required");
}
const fixture = new URL(jwksUrl);
if (fixture.protocol !== "http:" || fixture.hostname !== "127.0.0.1") {
  throw new Error("JWKS fixture must be an owned loopback HTTP endpoint");
}
const { Miniflare, Response, Log, LogLevel, convertV4MiniflareOptions } = await import(pathToFileURL(miniflareEntrypoint).href);
const runtime = new Miniflare(convertV4MiniflareOptions({
  host: "127.0.0.1",
  port: Number(portText),
  resourcePersistencePath: resolve(persistence),
  cf: false,
  log: new Log(LogLevel.INFO),
  workers: [{
  name: "collaboration-central-local-test",
  compatibilityDate: "2026-08-21",
  modulesRoot: resolve("crates/collaboration-central-cloudflare/build"),
  modules: [
    { type: "ESModule", path: resolve("crates/collaboration-central-cloudflare/build/index.js") },
    { type: "CompiledWasm", path: resolve("crates/collaboration-central-cloudflare/build/index_bg.wasm") },
  ],
  durableObjects: {
    CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
    PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
    LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
    FLEET: { className: "FleetObject", useSQLite: true },
  },
  bindings: {
    ACCESS_TEAM_DOMAIN: "conformance-test.cloudflareaccess.com",
    ACCESS_SESSION_AUD: "test-session-aud",
    ACCESS_ADMIN_AUD: "test-admin-aud",
    ACCESS_OWNER_IDENTITY: "owner@example.test",
  },
  cacheAPI: true,
  outboundService: async (request) => {
    if (request.url !== "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs") {
      throw new Error("Unexpected external fetch in isolated central test");
    }
    const response = await fetch(fixture);
    return new Response(await response.arrayBuffer(), {
      status: response.status,
      headers: response.headers,
    });
  },
  }],
}));

async function stop() {
  await runtime.dispose();
  process.exitCode = 0;
}
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
await runtime.ready;
process.stdout.write(`Ready on http://127.0.0.1:${portText} (real workerd, local JWKS redirect)\n`);
