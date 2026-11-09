// The guarded deploy (T1): configs per Worker kind, the inventory checks before
// and after, and the real runner over a fake cf that never uploads.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { guardedRun } from "../scripts/deploy_run.ts";
import {
  CENTRAL_CLASSES,
  DeployRefused,
  checkAfter,
  checkBefore,
  deployWorker,
  liveConfig,
  refuseProduction,
  renderConfig,
  type CfRunner,
  type DeploySettings,
  type Inventory,
  type WorkerKind,
} from "../scripts/worker_deploy.ts";

const SETTINGS: DeploySettings = {
  accountId: "0123456789abcdef0123456789abcdef",
  workerName: "collaboration-central-s1-migration",
  hostname: "s1-migration.staging.example",
  compatibilityDate: "2026-08-21",
  access: {
    teamDomain: "team.example",
    ownerIdentity: "owner@example.test",
    sessionAudience: "session-aud",
    adminAudience: "admin-aud",
  },
  central: {
    registryConfig: { authoritativeReceiver: "01a16eaa-ae36-7eec-9698-e8c34d10238b" },
    deliveryConfig: { relayPeer: "01a1c140-0000-7000-8000-000000000001" },
    holderAnswerTimeout: '{"secs":20,"nanos":0}',
    copyAckWait: '{"secs":20,"nanos":0}',
  },
};
const W = SETTINGS.workerName;
const ns = (id: string, cls: string, script = W) => ({
  id,
  name: `${script}_${cls}`,
  script,
  class: cls,
});
const FIVE = CENTRAL_CLASSES.map((cls, index) => ns(`id-${index}`, cls));
const OTHER = [ns("other-1", "FleetObject", "collaboration-central-staging-one-core")];
const BOUND = [{ hostname: SETTINGS.hostname, service: W }];
const inventory = (
  namespaces: Inventory["namespaces"],
  exists: boolean,
  domains: Inventory["domains"],
): Inventory => ({
  namespaces,
  workers: exists ? [{ id: W }] : [],
  domains,
});
const refusedWith = (pattern: RegExp) => (error: unknown) =>
  error instanceof DeployRefused && pattern.test(error.message);

test("each central kind renders the five-class shape, migration's entry, and step 5's tombstones", () => {
  const old = renderConfig("old-central", SETTINGS);
  assert.deepEqual(Object.keys(old.worker.exports).sort(), [...CENTRAL_CLASSES].sort());
  assert.deepEqual(old.worker.domains, [SETTINGS.hostname]);
  assert.equal(old.worker.entrypoint, "artifact/index.js");
  assert.equal(
    (old.worker.env.CENTRAL_REGISTRY_CONFIG as unknown as { value: string }).value,
    JSON.stringify(SETTINGS.central.registryConfig),
  );
  assert.equal(renderConfig("migration", SETTINGS).worker.entrypoint, "artifact/worker.js");
  const main = renderConfig("main-central", SETTINGS);
  assert.deepEqual(main.worker.exports.LeaseRegistryObject, {
    type: "durable-object",
    state: "deleted",
  });
  assert.deepEqual(main.worker.exports.FleetObject, { type: "durable-object", storage: "sqlite" });
  assert.equal(main.worker.env.LEASE_REGISTRY, undefined);
  assert.notEqual(main.worker.env.FLEET, undefined);
});

test("production's name or domain is refused", () => {
  assert.throws(
    () => refuseProduction(liveConfig({ ...SETTINGS, workerName: "collaboration-central" }, true)),
    refusedWith(/production/),
  );
  assert.throws(
    () =>
      refuseProduction(
        liveConfig({ ...SETTINGS, hostname: "central.agent-collaboration.com" }, true),
      ),
    refusedWith(/production/),
  );
});

test("the seed's first upload refuses an existing Worker, existing namespaces or an existing hostname", () => {
  checkBefore("seed", SETTINGS, inventory(OTHER, false, []));
  assert.throws(
    () => checkBefore("seed", SETTINGS, inventory(OTHER, true, [])),
    refusedWith(/already exists/),
  );
  assert.throws(
    () => checkBefore("seed", SETTINGS, inventory([...OTHER, FIVE[0]!], false, [])),
    refusedWith(/already belong/),
  );
  assert.throws(
    () =>
      checkBefore(
        "seed",
        SETTINGS,
        inventory(OTHER, false, [{ hostname: SETTINGS.hostname, service: "x" }]),
      ),
    refusedWith(/hostname .* already exists/),
  );
});

test("a later deploy needs the Worker, its five namespaces and its hostname bound to it alone", () => {
  checkBefore("old-central", SETTINGS, inventory([...OTHER, ...FIVE], true, BOUND));
  assert.throws(
    () => checkBefore("migration", SETTINGS, inventory([...OTHER, ...FIVE], false, BOUND)),
    refusedWith(/doesn't exist/),
  );
  assert.throws(
    () => checkBefore("migration", SETTINGS, inventory([...OTHER, ...FIVE.slice(1)], true, BOUND)),
    refusedWith(/not the five central classes/),
  );
  assert.throws(
    () =>
      checkBefore(
        "migration",
        SETTINGS,
        inventory([...OTHER, ...FIVE], true, [{ hostname: SETTINGS.hostname, service: "x" }]),
      ),
    refusedWith(/isn't bound/),
  );
});

for (const [name, kind, before, after, pattern] of [
  [
    "the seed creates exactly five namespaces",
    "seed",
    inventory(OTHER, false, []),
    inventory([...OTHER, ...FIVE], true, BOUND),
    null,
  ],
  [
    "another Worker's namespace changes",
    "old-central",
    inventory([...OTHER, ...FIVE], true, BOUND),
    inventory(
      [ns("other-2", "FleetObject", "collaboration-central-staging-one-core"), ...FIVE],
      true,
      BOUND,
    ),
    /another Worker's namespaces changed/,
  ],
  [
    "a redeploy changes a namespace id",
    "migration",
    inventory([...OTHER, ...FIVE], true, BOUND),
    inventory([...OTHER, ...FIVE.slice(0, 4), ns("new", "MailboxObject")], true, BOUND),
    /namespace for MailboxObject changed id/,
  ],
  [
    "step 5 keeps Fleet and ProjectCopy and deletes the three",
    "main-central",
    inventory([...OTHER, ...FIVE], true, BOUND),
    inventory(
      [
        ...OTHER,
        ...FIVE.filter((namespace) =>
          ["ProjectCopyObject", "FleetObject"].includes(namespace.class),
        ),
      ],
      true,
      BOUND,
    ),
    null,
  ],
  [
    "step 5 keeps a class it should delete",
    "main-central",
    inventory([...OTHER, ...FIVE], true, BOUND),
    inventory(
      [...OTHER, ...FIVE.filter((namespace) => namespace.class !== "MailboxObject")],
      true,
      BOUND,
    ),
    /after deploy; expected/,
  ],
  [
    "step 5 loses the Fleet's namespace",
    "main-central",
    inventory([...OTHER, ...FIVE], true, BOUND),
    inventory([...OTHER, FIVE[0]!], true, BOUND),
    /after deploy; expected/,
  ],
] as const) {
  test(`after deploy: ${name}`, () => {
    const check = () => checkAfter(kind as WorkerKind, SETTINGS, before, after);
    if (pattern === null) check();
    else assert.throws(check, refusedWith(pattern));
  });
}

test("a dry run checks the inventory and runs cf's own dry run, and never uploads", async () => {
  // Arrange
  const calls: string[] = [];
  const answers = inventory([...OTHER, ...FIVE], true, BOUND);
  const cf: CfRunner = async (args) => {
    calls.push(args.join(" "));
    if (args[0] === "durable-objects") return JSON.stringify(answers.namespaces);
    if (args[0] === "workers" && args[1] === "scripts") return JSON.stringify(answers.workers);
    if (args[0] === "workers") return JSON.stringify(answers.domains);
    return "";
  };

  // Act
  const outcome = await deployWorker("migration", SETTINGS, cf, {
    dryRun: true,
    tag: "t",
    message: "m",
  });

  // Assert: nothing uploaded, and the planned diff keeps all five namespaces by id.
  assert.equal(outcome.after, null);
  assert.deepEqual(outcome.namespaces, {
    create: [],
    keep: FIVE.map(({ class: cls, id }) => ({ class: cls, id })),
    delete: [],
  });
  assert.deepEqual(
    calls.filter((call) => call.startsWith("deploy")),
    ["deploy --dry-run"],
  );
});

test("an inventory in an unknown shape is refused, not read as empty", async () => {
  const cf: CfRunner = async () => JSON.stringify({ success: false, errors: [] });
  await assert.rejects(
    deployWorker("old-central", SETTINGS, cf, { dryRun: true, tag: "t", message: "m" }),
    refusedWith(/unknown shape/),
  );
});

test("the real runner renders private files, uploads through cf once, and checks the after inventory", async () => {
  // Arrange: a fake cf whose inventory changes only once a real deploy has run.
  const root = await mkdtemp(join(tmpdir(), "2026-11-worker-deploy-"));
  try {
    const artifact = join(root, "build");
    await mkdir(artifact);
    await writeFile(join(artifact, "worker.js"), "export default {};\n");
    const statePath = join(root, "cf-state.json");
    await writeFile(
      statePath,
      JSON.stringify({
        log: [],
        deployed: false,
        before: inventory([...OTHER, ...FIVE], true, BOUND),
        after: inventory([...OTHER, ...FIVE], true, BOUND),
      }),
    );
    process.env.FAKE_CF_STATE = statePath;
    const scratch = join(root, "scratch");

    // Act
    const outcome = await guardedRun({
      kind: "migration",
      settings: SETTINGS,
      config: renderConfig("migration", SETTINGS),
      artifact,
      scratch,
      cf: join(import.meta.dirname, "support/fake_cf.mjs"),
      dryRun: false,
    });

    // Assert: one real deploy between the two inventories; every file private.
    const log = (JSON.parse(await readFile(statePath, "utf8")) as { log: string[][] }).log.map(
      (call) => call[0],
    );
    assert.equal(log.filter((call) => call === "deploy").length, 1);
    assert.ok(
      log.indexOf("deploy") > 0 && log.lastIndexOf("durable-objects") > log.indexOf("deploy"),
    );
    assert.equal(outcome.after?.namespaces.length, 6);
    assert.equal((await stat(scratch)).mode & 0o777, 0o700);
    for (const file of await readdir(scratch)) {
      if (file === "artifact") continue;
      assert.equal((await stat(join(scratch, file))).mode & 0o777, 0o600, file);
    }
    assert.match(
      await readFile(join(scratch, "cloudflare.config.ts"), "utf8"),
      /"entrypoint": "artifact\/worker.js"/,
    );
    assert.match(await readFile(join(scratch, "artifact/worker.js"), "utf8"), /export default/);
  } finally {
    delete process.env.FAKE_CF_STATE;
    await rm(root, { recursive: true, force: true });
  }
});

test("the runner refuses a cf token environment; cf must use the owner's login", async () => {
  process.env.CLOUDFLARE_API_TOKEN = "x";
  try {
    await assert.rejects(
      guardedRun({
        kind: "migration",
        settings: SETTINGS,
        config: renderConfig("migration", SETTINGS),
        artifact: "/nonexistent",
        scratch: "/nonexistent-scratch",
        cf: "cf",
        dryRun: true,
      }),
      /CLOUDFLARE_API_TOKEN is set/,
    );
  } finally {
    delete process.env.CLOUDFLARE_API_TOKEN;
  }
});

test("step 5's planned diff keeps Fleet and ProjectCopy and deletes the three retired classes", async () => {
  const { planNamespaces } = await import("../scripts/worker_deploy.ts");
  const plan = planNamespaces(
    "main-central",
    SETTINGS,
    inventory([...OTHER, ...FIVE], true, BOUND),
  );
  assert.deepEqual(plan.keep.map((entry) => entry.class).sort(), [
    "FleetObject",
    "ProjectCopyObject",
  ]);
  assert.deepEqual(plan.delete.map((entry) => entry.class).sort(), [
    "ConnectionObject",
    "LeaseRegistryObject",
    "MailboxObject",
  ]);
});
