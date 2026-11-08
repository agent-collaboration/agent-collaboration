// Local harness only: the compiled public central and central-link Workers run
// in real workerd, with a fixture caller Worker standing where sign-in sits.
// The caller reaches central-link only through its CENTRAL service binding;
// central-link reaches central's own FleetObject class through a Durable
// Object binding to the central script. Redirect exactly the fixture issuer's
// key fetches to the owned loopback servers.
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import publicTemplate from "../../crates/collaboration-central-cloudflare/cloudflare.config.ts";
import linkTemplate from "../../crates/collaboration-central-cloudflare/cloudflare-link.config.ts";

const [
  miniflareEntrypoint,
  persistence,
  portText,
  accessJwksUrl,
  signInJwksUrl,
  legacyOrganization,
  fixedAccessTime,
] = process.argv.slice(2);
if (
  !miniflareEntrypoint ||
  !persistence ||
  !portText ||
  !accessJwksUrl ||
  !signInJwksUrl ||
  !legacyOrganization ||
  !fixedAccessTime
) {
  throw new Error(
    "Miniflare entrypoint, isolated persistence, port, Access and sign-in fixture key URLs, legacy organization and fixed time required",
  );
}
for (const url of [accessJwksUrl, signInJwksUrl]) {
  const parsed = new URL(url);
  if (parsed.protocol !== "http:" || parsed.hostname !== "127.0.0.1")
    throw new Error("key fixtures must be owned loopback HTTP endpoints");
}
const ACCESS_CERTS = "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs";
const SIGN_IN_KEYS = "https://sign-in.example.invalid/api/auth/jwks";
const replacements = new Map(
  Object.entries({
    ACCESS_TEAM_DOMAIN: "conformance-test.cloudflareaccess.com",
    ACCESS_SESSION_AUD: "test-session-aud",
    ACCESS_ADMIN_AUD: "test-admin-aud",
    ACCESS_OWNER_IDENTITY: "owner@example.test",
    CENTRAL_HOLDER_ANSWER_TIMEOUT: '{"secs":2,"nanos":0}',
    CENTRAL_COPY_ACK_WAIT: '{"secs":2,"nanos":0}',
    CENTRAL_LEGACY_ORGANIZATION_ID: legacyOrganization,
    CENTRAL_REGISTRY_CONFIG: JSON.stringify({
      lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
      authoritativeReceiver: "019f0000-0000-7000-8000-000000000010",
      releaseUnsentFor: { secs: 0, nanos: 0 },
      moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
    }),
    CENTRAL_DELIVERY_CONFIG: JSON.stringify({ relayPeer: "019f0000-0000-7000-8000-00000000ad04" }),
    CENTRAL_SIGN_IN_ISSUER: "https://sign-in.example.invalid",
    CENTRAL_SIGN_IN_AUDIENCE: "https://central.example.invalid/identity",
    CENTRAL_SIGN_IN_JWKS_URL: SIGN_IN_KEYS,
    CENTRAL_ADMIN_AUDIENCE: "https://central.example.invalid/admin",
    CENTRAL_PUBLIC_ORIGIN: "https://central.example.invalid",
  }),
);
// Fill the shipped templates, so a missing binding cannot be masked by the
// harness's own list of settings.
function textBindings(env: Record<string, unknown>): Record<string, string> {
  const bindings: Record<string, string> = {};
  for (const [name, binding] of Object.entries(env)) {
    if (typeof binding !== "object" || binding === null) continue;
    if (!("type" in binding) || binding.type !== "text") continue;
    const value = replacements.get(name);
    if (value === undefined) throw new Error(`no fixture value for template variable ${name}`);
    bindings[name] = value;
  }
  return bindings;
}
const centralBindings = textBindings(publicTemplate.worker.env);
centralBindings.CENTRAL_CONFORMANCE_FIXED_TIME = fixedAccessTime;
const linkBindings = textBindings(linkTemplate.worker.env);
if (!("FLEET" in linkTemplate.worker.env) || "exports" in linkTemplate.worker)
  throw new Error("central-link template must bind FLEET and export no class");

async function redirectKeys(request: Request): Promise<unknown> {
  const target =
    request.url === ACCESS_CERTS
      ? accessJwksUrl
      : request.url === SIGN_IN_KEYS
        ? signInJwksUrl
        : undefined;
  if (target === undefined) throw new Error(`Unexpected external fetch: ${request.url}`);
  const response = await fetch(target);
  return new MiniflareResponse(await response.arrayBuffer(), {
    status: response.status,
    headers: response.headers,
  });
}

// The fixture caller: /link/* goes over the CENTRAL binding to central-link,
// /public/* over a binding to public central; nothing else answers.
const callerScript = `
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    for (const [prefix, target, origin] of [
      ["/link", env.CENTRAL, "http://central.invalid"],
      ["/public", env.PUBLIC, "https://central.example.invalid"],
    ]) {
      if (url.pathname.startsWith(prefix + "/")) {
        const forwarded = new Request(origin + url.pathname.slice(prefix.length) + url.search, request);
        return target.fetch(forwarded);
      }
    }
    return new Response(null, { status: 404 });
  },
};`;

const { Miniflare, Response: MiniflareResponse, Log, LogLevel, convertV4MiniflareOptions } =
  await import(pathToFileURL(miniflareEntrypoint).href);
const artifact = (directory: string) => ({
  modulesRoot: resolve(`crates/collaboration-central-cloudflare/${directory}`),
  modules: [
    {
      type: "ESModule",
      path: resolve(`crates/collaboration-central-cloudflare/${directory}/index.js`),
    },
    {
      type: "CompiledWasm",
      path: resolve(`crates/collaboration-central-cloudflare/${directory}/index_bg.wasm`),
    },
  ],
});
const runtime = new Miniflare(
  convertV4MiniflareOptions({
    host: "127.0.0.1",
    port: Number(portText),
    resourcePersistencePath: resolve(persistence),
    cf: false,
    log: new Log(LogLevel.INFO),
    workers: [
      {
        name: "sign-in-fixture",
        compatibilityDate: "2026-08-21",
        modules: true,
        script: callerScript,
        serviceBindings: { CENTRAL: "central-link", PUBLIC: "central" },
      },
      {
        name: "central",
        compatibilityDate: "2026-08-21",
        ...artifact("build"),
        durableObjects: {
          PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
          FLEET: { className: "FleetObject", useSQLite: true },
          SCHEMA_PROBES: { className: "BootstrapObject", useSQLite: true },
        },
        bindings: centralBindings,
        outboundService: redirectKeys,
      },
      {
        name: "central-link",
        compatibilityDate: "2026-08-21",
        ...artifact("build-link"),
        durableObjects: {
          FLEET: { className: "FleetObject", scriptName: "central", useSQLite: true },
        },
        bindings: linkBindings,
        outboundService: redirectKeys,
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
process.stdout.write(`Ready on http://127.0.0.1:${portText} (real workerd, central-link binding)\n`);
