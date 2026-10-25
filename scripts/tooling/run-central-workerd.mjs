// Local harness only: the compiled Rust Worker and workerd APIs remain real.
// Redirect exactly the fixture issuer's certs fetch to the owned HTTP server.
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import publicTemplate from "../../crates/collaboration-central-cloudflare/cloudflare.config.ts";

const [miniflareEntrypoint, persistence, portText, jwksUrl, holderTimeout, copyWait, relayPeer] =
  process.argv.slice(2);
if (
  !miniflareEntrypoint ||
  !persistence ||
  !portText ||
  !jwksUrl ||
  !holderTimeout ||
  !copyWait ||
  !relayPeer
) {
  throw new Error(
    "Miniflare entrypoint, isolated persistence, port, fixture JWKS URL, explicit holder/copy timeout JSON and relay peer required",
  );
}
const fixture = new URL(jwksUrl);
if (fixture.protocol !== "http:" || fixture.hostname !== "127.0.0.1") {
  throw new Error("JWKS fixture must be an owned loopback HTTP endpoint");
}
// Fill the shipped template, so a missing public binding cannot be masked by
// the harness's own independent list of production settings.
const replacements = {
  ACCESS_TEAM_DOMAIN: "conformance-test.cloudflareaccess.com",
  ACCESS_SESSION_AUD: "test-session-aud",
  ACCESS_ADMIN_AUD: "test-admin-aud",
  ACCESS_OWNER_IDENTITY: "owner@example.test",
  CENTRAL_HOLDER_ANSWER_TIMEOUT: holderTimeout,
  CENTRAL_COPY_ACK_WAIT: copyWait,
  CENTRAL_REGISTRY_CONFIG: JSON.stringify({
    lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
    authoritativeReceiver: "019f0000-0000-7000-8000-000000000010",
    releaseUnsentFor: { secs: 0, nanos: 0 },
    moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
  }),
  CENTRAL_DELIVERY_CONFIG: relayPeer.startsWith("{") ? relayPeer : JSON.stringify({ relayPeer }),
};
const templateBindings = Object.fromEntries(
  Object.entries(publicTemplate.worker.env)
    .filter(([, binding]) => binding.type === "text")
    .map(([name, binding]) => [name, replacements[name] ?? binding.value]),
);
for (const name of Object.keys(replacements)) {
  if (!(name in templateBindings))
    throw new Error(`public central template missing required ${name}`);
}
const { Miniflare, Response, Log, LogLevel, convertV4MiniflareOptions } = await import(
  pathToFileURL(miniflareEntrypoint).href
);
const runtime = new Miniflare(
  convertV4MiniflareOptions({
    host: "127.0.0.1",
    port: Number(portText),
    resourcePersistencePath: resolve(persistence),
    cf: false,
    log: new Log(LogLevel.INFO),
    workers: [
      {
        name: "collaboration-central-local-test",
        compatibilityDate: "2026-08-21",
        modulesRoot: resolve("crates/collaboration-central-cloudflare/build"),
        modules: [
          {
            type: "ESModule",
            path: resolve("crates/collaboration-central-cloudflare/build/index.js"),
          },
          {
            type: "CompiledWasm",
            path: resolve("crates/collaboration-central-cloudflare/build/index_bg.wasm"),
          },
        ],
        durableObjects: {
          CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
          PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
          LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
          FLEET: { className: "FleetObject", useSQLite: true },
          MAILBOXES: { className: "MailboxObject", useSQLite: true },
          SCHEMA_PROBES: { className: "BootstrapObject", useSQLite: true },
        },
        bindings: templateBindings,
        cacheAPI: true,
        outboundService: async (request) => {
          if (
            request.url !== "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs"
          ) {
            throw new Error("Unexpected external fetch in isolated central test");
          }
          const response = await fetch(fixture);
          return new Response(await response.arrayBuffer(), {
            status: response.status,
            headers: response.headers,
          });
        },
      },
    ],
  }),
);

async function stop() {
  await runtime.dispose();
  process.exitCode = 0;
}
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
await runtime.ready;
process.stdout.write(`Ready on http://127.0.0.1:${portText} (real workerd, local JWKS redirect)\n`);
