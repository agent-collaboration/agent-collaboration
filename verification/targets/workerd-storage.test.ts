import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer, type Server } from "node:http";
import { copyFile, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { WorkerdStorage, type WorkerdBundle } from "./workerd-storage.ts";

const sourceUrl = "https://signin.test/.well-known/jwks.json";

test("bridge names every absent public or private artifact before readiness", async (): Promise<void> => {
  const fixture = createFixtureDirectory("workerd-missing-bundle");
  const repository = fileURLToPath(new URL("../../", import.meta.url));
  const bridgePath = "crates/collaboration-central-cloudflare/tests/support/workerd-bridge.mts";
  try {
    await mkdir(join(fixture, dirname(bridgePath)), { recursive: true });
    await mkdir(join(fixture, "verification/targets"), { recursive: true });
    await writeFile(join(fixture, "package.json"), '{"type":"module"}');
    await copyFile(join(repository, bridgePath), join(fixture, bridgePath));
    await copyFile(
      join(repository, "verification/targets/workerd-storage.ts"),
      join(fixture, "verification/targets/workerd-storage.ts"),
    );
    await symlink(
      join(repository, "verification/node_modules"),
      join(fixture, "verification/node_modules"),
      "dir",
    );
    const plan = join(fixture, "plan.json");
    await writeFile(
      plan,
      JSON.stringify({
        textBindings: { CENTRAL_SIGN_IN_JWKS_URL: sourceUrl },
        jwksFixtureUrl: "http://127.0.0.1:12345/jwks",
      }),
    );
    for (const directory of ["build-test-support", "build-link-test-support"]) {
      await mkdir(join(fixture, "crates/collaboration-central-cloudflare", directory));
    }
    const paths = [
      "build-test-support/index.js",
      "build-test-support/index_bg.wasm",
      "build-link-test-support/index.js",
      "build-link-test-support/index_bg.wasm",
    ];
    for (const missing of paths) {
      for (const path of paths) {
        const absolute = join(fixture, "crates/collaboration-central-cloudflare", path);
        if (path === missing) await rm(absolute, { force: true });
        else await writeFile(absolute, "fixture must never start");
      }
      const result = spawnSync(
        process.execPath,
        [join(fixture, bridgePath), "--signin-plan", plan],
        { encoding: "utf8" },
      );
      assert.equal(result.status, 1);
      assert.equal(result.stdout, "", "missing artifacts must never publish readiness");
      assert.equal(
        result.stderr.trim(),
        `WORKERD_BUNDLE_MISSING ${JSON.stringify({ path: join(fixture, "crates/collaboration-central-cloudflare", missing) })}`,
      );
    }
  } finally {
    await rm(fixture, { recursive: true });
  }
});
const publicSource = `
export class CentralObject {
  constructor(state) { this.state = state; }
  async fetch(request) {
    const count = (await this.state.storage.get("count") ?? 0) + 1;
    await this.state.storage.put("count", count);
    return Response.json({ count, path: new URL(request.url).pathname,
      method: request.method, body: await request.text(), proof: request.headers.get("x-proof") },
      { status: 207 });
  }
}
export default { async fetch(request, env) {
  const url = new URL(request.url);
  if (url.pathname === "/counter")
    return env.CENTRAL.get(env.CENTRAL.idFromName("same-central")).fetch(request);
  if (url.pathname === "/outbound") {
    try { return await fetch(url.searchParams.get("url"), { method: request.method, redirect: "manual",
      headers: { authorization: "fixture-only", cookie: "fixture-only", "x-proof": "fixture-only" } }); }
    catch { return new Response("outbound refused", { status: 418 }); }
  }
  return new Response("public missing", { status: 404 });
} };
`;
const linkSource = `export default { async fetch(request, env) {
  if (new URL(request.url).pathname !== "/counter") return new Response("link missing", { status: 404 });
  return env.CENTRAL.get(env.CENTRAL.idFromName("same-central")).fetch(request);
} };`;

let root: string;
let server: Server;
let host: WorkerdStorage;
let publicBundle: WorkerdBundle;
let linkBundle: WorkerdBundle;
let fixtureUrl: string;
let document = "first-key-document";
let redirect = false;
const arrivals: Array<{
  method: string | undefined;
  url: string | undefined;
  credential: boolean;
}> = [];

async function bundle(name: string, source: string): Promise<WorkerdBundle> {
  const directory = join(root, name);
  await mkdir(directory);
  await writeFile(join(directory, "index.js"), source);
  await writeFile(join(directory, "index_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  return {
    bundleDirectory: directory,
    buildCommand: "test fixture",
    compatibilityDate: "2026-08-21",
    durableObjects: name === "public" ? { CENTRAL: "CentralObject" } : {},
    textBindings: {},
  };
}

before(async () => {
  root = createFixtureDirectory("workerd-signin-apparatus");
  publicBundle = await bundle("public", publicSource);
  linkBundle = await bundle("link", linkSource);
  server = createServer((request, response) => {
    arrivals.push({
      method: request.method,
      url: request.url,
      credential:
        request.headers.authorization !== undefined ||
        request.headers.cookie !== undefined ||
        request.headers["x-proof"] !== undefined,
    });
    if (redirect) {
      response.writeHead(302, { location: "/not-a-jwks-fixture" });
      response.end();
      return;
    }
    response.writeHead(203, { "content-type": "application/json", "x-fixture": "preserved" });
    response.end(document);
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (address === null || typeof address === "string")
    throw new Error("fixture has no TCP address");
  fixtureUrl = `http://127.0.0.1:${address.port}/jwks`;
  host = await WorkerdStorage.start(publicBundle, { linkBundle, jwks: { sourceUrl, fixtureUrl } });
});

after(async () => {
  await host?.close();
  if (server !== undefined)
    await new Promise<void>((resolve, reject) => {
      server.close((error) => (error === undefined ? resolve() : reject(error)));
    });
  if (root !== undefined) await rm(root, { recursive: true });
});

test("private service binding and public worker share the same durable namespace", async (): Promise<void> => {
  const first = await fetch(`${host.origin}/public/counter?keep=yes`, {
    method: "POST",
    body: "public bytes",
    headers: { "x-proof": "public proof" },
  });
  assert.equal(first.status, 207);
  assert.deepEqual(await first.json(), {
    count: 1,
    path: "/counter",
    method: "POST",
    body: "public bytes",
    proof: "public proof",
  });
  const second = await fetch(`${host.origin}/private/counter?keep=yes`, {
    method: "POST",
    body: "private bytes",
    headers: { "x-proof": "private proof" },
  });
  assert.equal(second.status, 207);
  assert.deepEqual(await second.json(), {
    count: 2,
    path: "/counter",
    method: "POST",
    body: "private bytes",
    proof: "private proof",
  });
  assert.equal((await fetch(`${host.origin}/other/counter`)).status, 404);
});

test("JWKS transport forwards each exact GET without credentials and preserves rotation and status", async (): Promise<void> => {
  const invoke = (): Promise<Response> =>
    fetch(`${host.origin}/public/outbound?url=${encodeURIComponent(sourceUrl)}`);
  const first = await invoke();
  assert.equal(first.status, 203);
  assert.equal(first.headers.get("x-fixture"), "preserved");
  assert.equal(await first.text(), document);
  document = "rotated-key-document";
  const second = await invoke();
  assert.equal(second.status, 203);
  assert.equal(await second.text(), document);
  assert.deepEqual(arrivals, [
    { method: "GET", url: "/jwks", credential: false },
    { method: "GET", url: "/jwks", credential: false },
  ]);
});

test("wrong JWKS URL and method never reach the fixture", async (): Promise<void> => {
  const before = arrivals.length;
  for (const url of [sourceUrl + "?unexpected=1", "https://other.test/jwks"]) {
    const response = await fetch(`${host.origin}/public/outbound?url=${encodeURIComponent(url)}`);
    assert.equal(response.status, 500);
  }
  const post = await fetch(`${host.origin}/public/outbound?url=${encodeURIComponent(sourceUrl)}`, {
    method: "POST",
  });
  assert.equal(post.status, 500);
  assert.equal(arrivals.length, before);
});

test("JWKS fixture redirects are returned without following another fixture path", async (): Promise<void> => {
  const before = arrivals.length;
  redirect = true;
  try {
    const response = await fetch(
      `${host.origin}/public/outbound?url=${encodeURIComponent(sourceUrl)}`,
      { redirect: "manual" },
    );
    assert.equal(response.status, 302);
    assert.equal(response.headers.get("location"), "/not-a-jwks-fixture");
    assert.equal(arrivals.length, before + 1);
  } finally {
    redirect = false;
  }
});

test("default loader still refuses every outbound fetch", async (): Promise<void> => {
  const plain = await WorkerdStorage.start(publicBundle);
  try {
    const before = arrivals.length;
    const response = await fetch(`${plain.origin}/outbound?url=${encodeURIComponent(sourceUrl)}`);
    assert.equal(response.status, 500);
    assert.equal(arrivals.length, before);
  } finally {
    await plain.close();
  }
});

test("invalid outbound configuration refuses before starting workers", async (): Promise<void> => {
  for (const invalid of [
    { sourceUrl: "http://signin.test/jwks", fixtureUrl },
    { sourceUrl, fixtureUrl: "http://localhost:12345/jwks" },
    { sourceUrl, fixtureUrl: "http://127.0.0.1:0/jwks" },
    { sourceUrl, fixtureUrl: "http://user:password@127.0.0.1:12345/jwks" },
    { sourceUrl: sourceUrl + "#fragment", fixtureUrl },
  ])
    await assert.rejects(WorkerdStorage.start(publicBundle, { linkBundle, jwks: invalid }));
});

test("private link cannot silently create its own central namespace", async (): Promise<void> => {
  await assert.rejects(
    WorkerdStorage.start(publicBundle, {
      linkBundle: { ...linkBundle, durableObjects: { CENTRAL: "CentralObject" } },
      jwks: { sourceUrl, fixtureUrl },
    }),
    /must not own a separate/,
  );
  await assert.rejects(
    WorkerdStorage.start(
      { ...publicBundle, durableObjects: {} },
      {
        linkBundle,
        jwks: { sourceUrl, fixtureUrl },
      },
    ),
    /requires the public CENTRAL/,
  );
});
