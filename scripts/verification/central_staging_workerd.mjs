// Five public SQLite bindings and the four production-duration settings.
// Only the fixture issuer's JWKS fetch is redirected; no external fetch passes.
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import publicTemplate from "../../crates/collaboration-central-cloudflare/cloudflare.config.ts";

const [miniflarePath, bundle, persistence, port, jwksUrl] = process.argv.slice(2);
if (!miniflarePath || !bundle || !persistence || !port || !jwksUrl)
  throw new Error("miniflare, bundle, persistence, port and fixture JWKS URL required");
const fixture = new URL(jwksUrl);
if (fixture.protocol !== "http:" || fixture.hostname !== "127.0.0.1")
  throw new Error("fixture JWKS must be loopback");
const replacement = {
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
  CENTRAL_DELIVERY_CONFIG: JSON.stringify({ relayPeer: "019f0000-0000-7000-8000-000000000011" }),
};
const bindings = Object.fromEntries(Object.entries(publicTemplate.worker.env)
  .filter(([, binding]) => binding.type === "text")
  .map(([name, binding]) => [name, replacement[name] ?? binding.value]));
for (const name of Object.keys(replacement))
  if (!(name in bindings)) throw new Error(`missing public setting ${name}`);
const objects = Object.fromEntries(Object.entries(publicTemplate.worker.env)
  .filter(([, binding]) => binding.type === "durable-object")
  .map(([name, binding]) => [name, { className: binding.exportName, useSQLite: true }]));
if (Object.keys(objects).length !== 5) throw new Error("expected five public object bindings");
const { Miniflare, Response, Log, LogLevel, convertV4MiniflareOptions } = await import(pathToFileURL(miniflarePath).href);
// Miniflare's Log writes to stdout by default. Keep the one ready protocol
// frame on stdout and all runtime diagnostics in the parent's private stderr.
class EvidenceLog extends Log {
  log(message) { process.stderr.write(`${message}\n`); }
}
const runtime = new Miniflare(convertV4MiniflareOptions({
  host: "127.0.0.1", port: Number(port), resourcePersistencePath: resolve(persistence), cf: false,
  log: new EvidenceLog(LogLevel.INFO), workers: [{
    name: "central-staging-ob7-local", compatibilityDate: publicTemplate.worker.compatibilityDate,
    modulesRoot: resolve(bundle), modules: [
      { type: "ESModule", path: resolve(bundle, "index.js") },
      { type: "CompiledWasm", path: resolve(bundle, "index_bg.wasm") },
    ], durableObjects: objects, bindings, cacheAPI: true,
    outboundService: async (request) => {
      if (request.url !== "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs")
        throw new Error("unexpected external fixture fetch");
      const reply = await fetch(fixture);
      return new Response(await reply.arrayBuffer(), { status: reply.status, headers: reply.headers });
    },
  }],
}));
let stopping = false;
async function stop() { if (!stopping) { stopping = true; await runtime.dispose(); process.exitCode = 0; } }
process.once("SIGTERM", stop);
process.once("SIGINT", stop);
await runtime.ready;
process.stdout.write(JSON.stringify({ kind: "workerdReady", origin: `http://127.0.0.1:${port}`, pid: process.pid }) + "\n");
