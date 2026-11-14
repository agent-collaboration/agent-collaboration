import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createServer, type Server } from "node:http";
import { copyFile, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { createInterface } from "node:readline";
import { once } from "node:events";
import { z } from "zod";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import {
  WorkerdStorage,
  centralTestBundle,
  workerdLauncherTimeSchema,
  type WorkerdBundle,
} from "./workerd-storage.ts";

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
  constructor(state, env) { this.state = state; this.env = env; }
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/clock-presence")
      return Response.json(this.env.TEST_SIGNIN_CLOCK !== undefined);
    if (url.pathname === "/fixture-control") {
      if (url.searchParams.get("operation") === "clock" &&
          url.searchParams.get("organization").endsWith("d214"))
        return new Response("fixture push failure", {status: 503});
      const clock = await this.env.TEST_SIGNIN_CLOCK.fetch(
        "https://signin-clock.test/clock/" + url.searchParams.get("organization"));
      const {milliseconds} = await clock.json();
      if (milliseconds !== null) await this.state.storage.put("milliseconds", milliseconds);
      if (url.searchParams.get("operation") === "clock") {
        await this.state.storage.put("milliseconds", await request.json());
        return Response.json(true);
      }
      return Response.json({complete: true,
        fixtureMilliseconds: await this.state.storage.get("milliseconds") ?? null});
    }
    if (url.pathname === "/clock") {
      const clockUrl = url.searchParams.get("clockUrl") ??
        "https://signin-clock.test/clock/" + url.searchParams.get("organization");
      return this.env.TEST_SIGNIN_CLOCK.fetch(new Request(clockUrl,
        { method: url.searchParams.get("method") ?? "GET" }));
    }
    const count = (await this.state.storage.get("count") ?? 0) + 1;
    await this.state.storage.put("count", count);
    return Response.json({ count, path: new URL(request.url).pathname,
      method: request.method, body: await request.text(), proof: request.headers.get("x-proof") },
      { status: 207 });
  }
}
export default { async fetch(request, env) {
  const url = new URL(request.url);
  const control = url.pathname.match(/^\\/test-support\\/workspace\\/([^/]+)\\/(clock|raw-signin-observe)$/);
  if (control) {
    const target = "https://fixture.test/fixture-control?organization=" + control[1] +
      "&operation=" + control[2];
    return env.CENTRAL.get(env.CENTRAL.idFromName(control[1])).fetch(new Request(target, request));
  }
  if (["/counter", "/clock", "/clock-presence"].includes(url.pathname))
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
  if (!["/counter", "/clock", "/clock-presence"].includes(new URL(request.url).pathname)) return new Response("link missing", { status: 404 });
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

const bridgeRelativePath =
  "crates/collaboration-central-cloudflare/tests/support/workerd-bridge.mts";
const emptyObservationPath = "crates/collaboration-central-core/src/signin_observation_empty.json";
const bridgeAnswerSchema = z.object({ answer: z.string() }).catchall(z.json());
type BridgeControl = (command: unknown) => Promise<z.infer<typeof bridgeAnswerSchema>>;

async function assertBridgeControls(props: {
  readonly baseUrl: string;
  readonly control: BridgeControl;
  readonly empty: unknown;
  readonly launcher: boolean;
}): Promise<void> {
  const { baseUrl, control, empty, launcher } = props;
  const presence = await fetch(`${baseUrl}/clock-presence`);
  assert.equal(await presence.json(), launcher, "only launcher mode installs the clock binding");
  if (!launcher) {
    assert.equal((await control({ setTimeMs: 1_900_000_000_000 })).answer, "refused");
    assert.equal(
      (await control({ observe: "019f0000-0000-7000-8000-00000000d211" })).answer,
      "refused",
    );
    return;
  }
  const organization = "019f0000-0000-7000-8000-00000000d211";
  const later = "019f0000-0000-7000-8000-00000000d212";
  const probe = "019f0000-0000-7000-8000-00000000d213";
  const touch = async (id: string): Promise<unknown> => {
    const response = await fetch(`${baseUrl}/test-support/workspace/${id}/raw-signin-observe`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
    });
    assert.equal(response.status, 200);
    return response.json();
  };
  // Unknown observation must neither create an object nor register it as live.
  assert.deepEqual(await control({ observe: organization }), {
    answer: "observed",
    organization,
    observation: empty,
  });
  assert.deepEqual(await touch(probe), { complete: true, fixtureMilliseconds: 1_800_000_000_000 });
  const milliseconds = 1_900_000_000_000;
  assert.deepEqual(await control({ setTimeMs: milliseconds }), {
    answer: "timeSet",
    setTimeMs: milliseconds,
    liveFleets: 1,
  });
  assert.deepEqual(await touch(organization), {
    complete: true,
    fixtureMilliseconds: milliseconds,
  });
  assert.deepEqual(await control({ setTimeMs: milliseconds + 5000 }), {
    answer: "timeSet",
    setTimeMs: milliseconds + 5000,
    liveFleets: 2,
  });
  assert.deepEqual(await control({ observe: organization }), {
    answer: "observed",
    organization,
    observation: { complete: true, fixtureMilliseconds: milliseconds + 5000 },
  });
  assert.deepEqual(await touch(later), {
    complete: true,
    fixtureMilliseconds: milliseconds + 5000,
  });
  assert.equal((await control({ restartFresh: true })).answer, "refused");
  for (const invalid of [1.5, -1, 9_223_372_036_855, Number.MAX_SAFE_INTEGER, null, "123"]) {
    assert.equal((await control({ setTimeMs: invalid })).answer, "refused");
    assert.deepEqual(await touch(organization), {
      complete: true,
      fixtureMilliseconds: milliseconds + 5000,
    });
  }
  const untouched = "019f0000-0000-7000-8000-00000000d215";
  assert.deepEqual(await touch(untouched), {
    complete: true,
    fixtureMilliseconds: milliseconds + 5000,
  });
  await touch("019f0000-0000-7000-8000-00000000d214");
  assert.deepEqual(await control({ setTimeMs: milliseconds + 10000 }), {
    answer: "failed",
    error: "bridge time may be partly applied",
  });
}

async function createBridgeControlFixture(): Promise<string> {
  const repository = fileURLToPath(new URL("../../", import.meta.url));
  const fixture = createFixtureDirectory("workerd-bridge-controls");
  await mkdir(join(fixture, dirname(bridgeRelativePath)), { recursive: true });
  await mkdir(join(fixture, dirname(emptyObservationPath)), { recursive: true });
  await mkdir(join(fixture, "verification/targets"), { recursive: true });
  await writeFile(join(fixture, "package.json"), '{"type":"module"}');
  for (const path of [
    bridgeRelativePath,
    emptyObservationPath,
    "verification/targets/workerd-storage.ts",
  ])
    await copyFile(join(repository, path), join(fixture, path));
  await symlink(
    join(repository, "verification/node_modules"),
    join(fixture, "verification/node_modules"),
    "dir",
  );
  for (const [directory, source] of [
    ["build-test-support", publicSource],
    ["build-link-test-support", linkSource],
  ]) {
    assert.ok(directory !== undefined && source !== undefined);
    const target = join(fixture, "crates/collaboration-central-cloudflare", directory);
    await mkdir(target);
    await writeFile(join(target, "index.js"), source);
    await writeFile(join(target, "index_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  }
  await writeFile(
    join(fixture, "plan.json"),
    JSON.stringify({
      textBindings: { CENTRAL_SIGN_IN_JWKS_URL: sourceUrl },
      jwksFixtureUrl: fixtureUrl,
    }),
  );
  return fixture;
}

test("launcher rejects invalid initial time before readiness or object creation", async (): Promise<void> => {
  const fixture = await createBridgeControlFixture();
  try {
    for (const initial of ["1.5", "-1", "9223372036855", "NaN", "", undefined]) {
      const result = spawnSync(
        process.execPath,
        [
          join(fixture, bridgeRelativePath),
          "--launcher-time-ms",
          ...(initial === undefined ? [] : [initial]),
        ],
        { encoding: "utf8" },
      );
      assert.equal(result.status, 1);
      assert.equal(result.stdout, "", "invalid time must not publish readiness");
      assert.match(result.stderr, /launcher time must be a supported integer millisecond/);
    }
    assert.equal(workerdLauncherTimeSchema.parse(0), 0);
    assert.equal(workerdLauncherTimeSchema.parse(9_223_372_036_854), 9_223_372_036_854);
  } finally {
    await rm(fixture, { recursive: true });
  }
});

test(
  "bridge IPC controls use the shared clock and external fresh-store restart",
  { timeout: 30_000 },
  async (): Promise<void> => {
    const repository = fileURLToPath(new URL("../../", import.meta.url));
    const empty: unknown = JSON.parse(
      await readFile(join(repository, emptyObservationPath), "utf8"),
    );
    // Each child has fresh persistence. These JS responses test transport only;
    // signed provisioning and actual core observations remain Rust-owned.
    for (const privateTopology of [false, true]) {
      for (let generation = 0; generation < 3; generation++) {
        const launcher = generation !== 0;
        const fixture = await createBridgeControlFixture();
        const plan = join(fixture, "plan.json");
        const child = spawn(
          process.execPath,
          [
            join(fixture, bridgeRelativePath),
            ...(privateTopology ? ["--signin-plan", plan] : []),
            ...(launcher ? ["--launcher-time-ms", "1800000000000"] : []),
          ],
          { stdio: "pipe" },
        );
        child.stdin.on("error", (error: NodeJS.ErrnoException): void => {
          if (error.code !== "EPIPE") throw error;
        });
        const exited = once(child, "close");
        const stdout = createInterface({ input: child.stdout });
        const stderr = createInterface({ input: child.stderr });
        const readyLines: string[] = [];
        stdout.on("line", (line: string): void => {
          readyLines.push(line);
        });
        const readiness = stdout[Symbol.asyncIterator]();
        const answers = stderr[Symbol.asyncIterator]();
        const control: BridgeControl = async (
          command: unknown,
        ): Promise<z.infer<typeof bridgeAnswerSchema>> => {
          child.stdin.write(`${JSON.stringify(command)}\n`);
          for (;;) {
            const line = await answers.next();
            assert.equal(line.done, false, "bridge exited before its JSON command answer");
            if (line.value === undefined) throw new Error("missing bridge answer line");
            try {
              const parsed = bridgeAnswerSchema.safeParse(JSON.parse(line.value));
              if (parsed.success) return parsed.data;
            } catch {
              /* Non-IPC diagnostics never become the command answer. */
            }
          }
        };
        let completed = false;
        try {
          const ready = await readiness.next();
          assert.equal(ready.done, false, "bridge did not publish readiness");
          if (ready.value === undefined) throw new Error("missing readiness line");
          const { origin } = z.object({ origin: z.url() }).strict().parse(JSON.parse(ready.value));
          const baseUrl = `${origin}${privateTopology ? "/public" : ""}`;
          await assertBridgeControls({ baseUrl, control, empty, launcher });
          assert.deepEqual(readyLines, [ready.value], "only one stdout readiness line");
          completed = true;
        } finally {
          if (child.exitCode === null && !child.stdin.destroyed) child.stdin.end("close\n");
          const [code] = await exited;
          stdout.close();
          stderr.close();
          await rm(fixture, { recursive: true });
          if (completed) {
            assert.equal(code, 0, "bridge close must settle the owned worker");
            assert.equal(readyLines.length, 1, "shutdown must not add stdout lines");
          }
        }
      }
    }
  },
);

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

test("optional clock binding reads one live cell without restarting durable state", async (): Promise<void> => {
  for (const privateTopology of [false, true]) {
    // Arrange: the Node clock and physical-org registry are outside the Worker.
    let milliseconds: number | null = null;
    const organizations: string[] = [];
    const first = "019f0000-0000-7000-8000-00000000d201";
    const second = "019f0000-0000-7000-8000-00000000d202";
    const controlled = await WorkerdStorage.start(
      publicBundle,
      privateTopology ? { linkBundle, jwks: { sourceUrl, fixtureUrl } } : undefined,
      (organization: string): number | null => {
        organizations.push(organization);
        return milliseconds;
      },
    );
    const origin = `${controlled.origin}${privateTopology ? "/private" : ""}`;
    const readClock = async (organization: string): Promise<unknown> => {
      const response = await fetch(`${origin}/clock?organization=${organization}`);
      assert.equal(response.status, 200);
      return response.json();
    };
    try {
      const before = await fetch(`${origin}/counter`);
      assert.deepEqual(await before.json(), {
        count: 1,
        path: "/counter",
        method: "GET",
        body: "",
        proof: null,
      });
      // Act: use the same Worker, then name an org only after time has moved.
      assert.deepEqual(await readClock(first), { milliseconds: null });
      milliseconds = 1_900_000_000_000;
      assert.deepEqual(await readClock(first), { milliseconds });
      assert.deepEqual(await readClock(second), { milliseconds });
      // Assert: the actual callback saw both orgs, and the durable object survived.
      assert.deepEqual(organizations, [first, first, second]);
      const after = await fetch(`${origin}/counter`);
      assert.deepEqual(await after.json(), {
        count: 2,
        path: "/counter",
        method: "GET",
        body: "",
        proof: null,
      });
      for (const query of [
        "organization=not-a-uuid",
        `organization=${first}&method=POST`,
        `clockUrl=${encodeURIComponent("https://other.test/clock/" + first)}`,
      ]) {
        const response = await fetch(`${origin}/clock?${query}`);
        assert.equal(response.status, 400);
      }
      assert.deepEqual(organizations, [first, first, second]);
    } finally {
      await controlled.close();
    }
  }
});

test("actual central samples launcher time once per object instance", async (): Promise<void> => {
  // The callback is the external clock boundary; the object and its latch are Rust/Wasm.
  const samples = new Map<string, number>();
  const bundle = centralTestBundle();
  const controlled = await WorkerdStorage.start(
    { ...bundle, durableObjects: { ...bundle.durableObjects, CENTRAL: "CentralObject" } },
    undefined,
    (organization: string): number => {
      samples.set(organization, (samples.get(organization) ?? 0) + 1);
      return 1_800_000_000_000;
    },
  );
  const first = "019f0000-0000-7000-8000-00000000d221";
  const second = "019f0000-0000-7000-8000-00000000d222";
  const path = (organization: string): string => `/test-support/workspace/${organization}`;
  try {
    assert.equal(await controlled.dispatchJson(`${path(first)}/clock`, 1_800_000_001_000), true);
    assert.equal(await controlled.dispatchJson(`${path(first)}/clock`, 1_800_000_002_000), true);
    assert.equal(await controlled.dispatchJson(`${path(second)}/clock`, 1_800_000_003_000), true);
    assert.deepEqual(
      [...samples],
      [
        [first, 1],
        [second, 1],
      ],
    );
    // The established crash control discards the instance, then the next touch samples again.
    await assert.rejects(controlled.dispatchJson(`${path(first)}/crash`, {}));
    assert.equal(await controlled.dispatchJson(`${path(first)}/clock`, 1_800_000_004_000), true);
    assert.deepEqual(
      [...samples],
      [
        [first, 2],
        [second, 1],
      ],
    );
  } finally {
    await controlled.close();
  }
});
