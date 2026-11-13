import assert from "node:assert/strict";
import { test } from "node:test";
import {
  mkdtemp,
  mkdir,
  writeFile,
  readFile,
  realpath,
  lstat,
  rm,
  appendFile,
  symlink,
} from "node:fs/promises";
import { createServer } from "node:net";
import { encode } from "../exact-json.ts";
import { Receipt } from "../receipt.ts";
import { fingerprint } from "./owned-process.ts";
import { Installation } from "./installation.ts";
import {
  snapshotExistingStagingHomes,
  validateExistingStagingHomes,
  validateStagingCodexHome,
} from "./codex-home.ts";
import { prepareStagingUserHome, validateStagingUserHome } from "./user-home-isolation.ts";
import {
  enrollInvocationsSince,
  readCommandLedger,
  type CommandLedgerEntry,
} from "./command-ledger.ts";
import {
  assertStoppedFileIdentities,
  loadStoppedOwnedRoot,
  validateOwnedStopReceipts,
  readStoppedObservations,
  type StoppedOwnedRootDeclaration,
} from "./installation-stopped-root.ts";

const project = "01900000-0000-7000-8000-000000000001";
const peer = "01900000-0000-7000-8000-000000000002";
const service = "01900000-0000-7000-8000-000000000003";
const marker = "01900000-0000-7000-8000-000000000004";
const commandIds = {
  host: "01900000-0000-7000-8000-000000000005",
  router: "01900000-0000-7000-8000-000000000006",
};
const at = "2026-11-10T00:00:00.000Z",
  settled = "2026-11-10T01:00:00.000Z";
async function fixture() {
  const parent = await realpath(await mkdtemp("/private/tmp/2026-11-s1-stop-"));
  const root = `${parent}/router`,
    evidence = `${parent}/evidence`,
    home = `${parent}/home`;
  await mkdir(root, { mode: 0o700 });
  await mkdir(evidence, { mode: 0o700 });
  const machine = await fixtureMachine(parent, root);
  const { router, cli } = machine;
  await fixtureHomes(machine, home, parent);
  await mkdir(`${root}/agent-communication/projects/${project}`, { recursive: true, mode: 0o700 });
  await reference(`${root}/agent-communication/projects/${project}/e1.sqlite`, "synthetic project");
  await reference(`${root}/agent-communication/service-identity.json`, {
    version: 1,
    serviceId: service,
  });
  await reference(`${root}/agent-communication/installation-peer.json`, {
    version: 1,
    peerId: peer,
  });
  await mkdir(`${root}/fleet`, { mode: 0o700 });
  await reference(`${root}/providers.json`, {
    version: 1,
    providers: {
      claude: { enabled: false, executable: null, arguments: [] },
      cursor: { enabled: false, executable: null, arguments: [] },
    },
  });
  const enrollment = await reference(`${root}/fleet/enrollment.json`, { synthetic: true });
  const baseline = {
    root,
    installationId: service,
    machineId: peer,
    endpoint: "https://staging.example.test/v1/session",
    enrollmentSha256: enrollment.sha256,
    baselineId: marker,
  };
  const { ledger, observations } = await fixtureStops(root, router.path);
  const original = {
    plan: await reference(`${parent}/original-plan.json`, {
      machine,
      stagingHome: home,
      repository: process.cwd(),
      sourceHead: "cbf56bad640ae981900c646ff2ae0b64aaaebd3b",
      productVersion: "0.1.125",
      managedCodex: cli,
      evidenceDirectory: evidence,
      files: [{ project, sourceRelative: `${project}/e1.sqlite` }],
    }),
    baseline: await reference(`${evidence}/baseline-for-upgrade.json`, baseline),
    hostWindow: await reference(`${evidence}/host-window.json`, { openedAt: at }),
    observations: await reference(
      `${evidence}/observations.jsonl`,
      observations.map((row) => encode(row)).join("\n") + "\n",
    ),
    cohort: await reference(`${parent}/cohort.json`, { projects: [{ directory: { project } }] }),
  };
  const info = await lstat(`${root}/agent-communication/projects/${project}/e1.sqlite`, {
    bigint: true,
  });
  const declaration: StoppedOwnedRootDeclaration = {
    kind: "stoppedOwnedRootContinuation",
    root,
    evidenceDirectory: `${parent}/continuation`,
    creatorUid: Number(info.uid),
    original,
    processes: {
      host: { pid: 900002, commandId: commandIds.host },
      router: { pid: 900001, commandId: commandIds.router },
    },
    execution: {
      productVersion: "0.1.125",
      sourceHead: "cbf56bad640ae981900c646ff2ae0b64aaaebd3b",
      router,
      cli,
    },
    controller: {
      sourceHead: "a".repeat(40),
      files: [await reference(`${parent}/controller.ts`, "synthetic controller")],
    },
    filesCapturedAt: "2026-11-10T02:00:00.000Z",
    files: [
      {
        project,
        sourceRelative: `${project}/e1.sqlite`,
        device: String(info.dev),
        inode: String(info.ino),
      },
    ],
    homes: await snapshotExistingStagingHomes({ machine, home }),
  };
  return { parent, root, machine, home, ledger, observations, declaration };
}

test("same-root composition keeps original marker and overlays execution without rewriting history", async () => {
  const f = await fixture();
  try {
    const originalText = await readFile(f.declaration.original.plan.path, "utf8");
    const overlay = {
      ...f.declaration.execution.cli,
      path: `${f.parent}/qualified-cli`,
      sha256: "b".repeat(64),
    };
    const context = await loadStoppedOwnedRoot({
      ...f.declaration,
      execution: { ...f.declaration.execution, cli: overlay },
    });
    assert.equal(context.machine.root, f.root);
    assert.deepEqual(context.machine.cli, overlay);
    assert.equal(context.baseline.baselineId, marker);
    assert.equal(await readFile(f.declaration.original.plan.path, "utf8"), originalText);
    validateOwnedStopReceipts({
      declaration: f.declaration,
      ledger: f.ledger,
      observations: f.observations,
      originalMachine: f.machine,
    });
    await assertStoppedFileIdentities(f.declaration);
    // The declared proof cohort is selected from an existing cache, not a claim
    // that the root has no other project files. Those are covered by lsof only.
    await writeFile(
      `${f.root}/agent-communication/projects/${project}/e2.sqlite`,
      "other existing cache file",
      { mode: 0o600 },
    );
    await assertStoppedFileIdentities(f.declaration);
    await validateExistingStagingHomes({
      machine: f.machine,
      home: f.home,
      expected: f.declaration.homes,
    });
    assert.equal(await enrollInvocationsSince(f.root, marker), 0);
    await assert.rejects(validateStagingCodexHome(f.machine), /state, sessions, auth/);
  } finally {
    await rm(f.parent, { recursive: true });
  }
});

for (const invalid of [
  "root",
  "enrollment",
  "enrollInvocation",
  "missingStop",
  "artifact",
  "activePort",
] as const)
  test(`stopped-root continueOld ${invalid} refusal launches no daemon`, async () => {
    const f = await fixture(),
      declaration = structuredClone(f.declaration);
    let listener: ReturnType<typeof createServer> | undefined;
    try {
      if (invalid === "root") declaration.root = `${f.parent}/other`;
      if (invalid === "enrollment") await writeFile(`${f.root}/fleet/enrollment.json`, "changed");
      if (invalid === "enrollInvocation") {
        const id = "01900000-0000-7000-8000-000000000007";
        await appendFile(
          `${f.root}/verification-commands.jsonl`,
          [
            {
              kind: "started",
              at,
              commandId: id,
              mode: "command",
              executable: f.machine.router.path,
              arguments: ["fleet", "enroll"],
              cwd: f.root,
              environmentKeys: [],
            },
            { kind: "ended", at, commandId: id, pid: 900003, exit: 0, signal: null, error: null },
          ]
            .map((row) => encode(row))
            .join("\n") + "\n",
        );
      }
      if (invalid === "missingStop")
        await writeFile(
          `${f.root}/verification-commands.jsonl`,
          f.ledger
            .filter((row) => row.kind !== "ended")
            .map((row) => encode(row))
            .join("\n") + "\n",
        );
      if (invalid === "artifact") await writeFile(f.machine.cli.path, "changed executable bytes");
      if (invalid === "activePort") {
        listener = createServer();
        await new Promise<void>((accept, reject) => {
          listener?.once("error", reject);
          listener?.listen(0, "127.0.0.1", accept);
        });
        const address = listener.address();
        assert.ok(address !== null && typeof address !== "string");
        // Keep the original and continuation declarations bound to the same allocated port.
        const original = {
          ...(await loadStoppedOwnedRoot(declaration)).original,
          machine: { ...f.machine, port: address.port },
        };
        await writeFile(declaration.original.plan.path, encode(original));
        declaration.original.plan.sha256 = await fingerprint(declaration.original.plan.path);
        // Port refusal precedes signature or version probes; configs retain the old port,
        // so update both minimal configs and their exact inventory to reach that guard.
        const profile = `${f.machine.codexHome}/codex-router-debug.config.toml`;
        await writeFile(
          profile,
          (await readFile(profile, "utf8")).replace(":18827/", `:${address.port}/`),
        );
        declaration.homes = await snapshotExistingStagingHomes({
          machine: { ...f.machine, port: address.port },
          home: f.home,
        });
      }
      const reasons = {
        root: /root or original creator/,
        enrollment: /fingerprint changed/,
        enrollInvocation: /enrollment occurred/,
        missingStop: /terminal ledger/,
        artifact: /overlay artifact changed/,
        activePort: /EADDRINUSE/,
      };
      await assert.rejects(
        Installation.continueStoppedOwned({ declaration, receipt: new Receipt(f.parent) }),
        reasons[invalid],
      );
      assert.equal(
        (await readCommandLedger(f.root)).filter(
          (row) => row.kind === "started" && row.mode === "daemon",
        ).length,
        2,
      );
    } finally {
      if (listener !== undefined)
        await new Promise<void>((accept, reject) =>
          listener?.close((error) => (error === undefined ? accept() : reject(error))),
        );
      await rm(f.parent, { recursive: true });
    }
  });

test("owned stop accepts raw Router SIGTERM but rejects abnormal or contradictory receipts", async () => {
  const f = await fixture();
  try {
    const check = (ledger: CommandLedgerEntry[], observations: unknown[] = f.observations) =>
      validateOwnedStopReceipts({
        declaration: f.declaration,
        ledger,
        observations,
        originalMachine: f.machine,
      });
    check(f.ledger);
    for (const signal of ["SIGKILL", "SIGINT"])
      assert.throws(
        () =>
          check(
            f.ledger.map((row) =>
              row.kind === "ended" && row.pid === 900001 ? { ...row, signal } : row,
            ),
          ),
        /normal owned stop/,
      );
    assert.throws(() => check(f.ledger, []), /stop observation/);
    assert.throws(
      () =>
        check(
          f.ledger.map((row) =>
            row.kind === "ended" && row.pid === 900002 ? { ...row, exit: 1 } : row,
          ),
        ),
      /normal owned stop/,
    );
  } finally {
    await rm(f.parent, { recursive: true });
  }
});

test("original journal reader hashes every byte while retaining exact stop observations", async () => {
  const f = await fixture();
  try {
    const path = `${f.parent}/journal.jsonl`;
    const ref = await reference(
      path,
      [{ kind: "other", value: "x".repeat(1024 * 1024) }, ...f.observations]
        .map((row) => encode(row))
        .join("\n") + "\n",
    );
    assert.deepEqual(await readStoppedObservations(ref), f.observations);
    await appendFile(path, encode({ kind: "other", value: "changed ignored row" }) + "\n");
    await assert.rejects(readStoppedObservations(ref), /journal fingerprint changed/);
  } finally {
    await rm(f.parent, { recursive: true });
  }
});

test("existing native SQLite sidecars require known bases, regular private files and an exact inventory", async () => {
  const f = await fixture();
  try {
    for (const base of ["goals_1", "logs_2", "memories_1", "queue_1", "state_5"])
      for (const suffix of ["wal", "shm"])
        await writeFile(
          `${f.machine.codexHome}/${base}.sqlite-${suffix}`,
          `synthetic ${base} ${suffix}`,
          { mode: 0o600 },
        );
    const expected = await snapshotExistingStagingHomes({ machine: f.machine, home: f.home });
    await validateExistingStagingHomes({ machine: f.machine, home: f.home, expected });
    await assert.rejects(validateStagingCodexHome(f.machine), /state, sessions, auth/);
    const sidecar = `${f.machine.codexHome}/state_5.sqlite-wal`;
    const { chmod } = await import("node:fs/promises");
    await chmod(sidecar, 0o644);
    await assert.rejects(
      snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
      /private SQLite sidecar/,
    );
    await rm(sidecar);
    await mkdir(sidecar, { mode: 0o700 });
    await assert.rejects(
      snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
      /private SQLite sidecar/,
    );
    await rm(sidecar, { recursive: true });
    await symlink(`${f.machine.codexHome}/state_5.sqlite`, sidecar);
    await assert.rejects(
      snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
      /private SQLite sidecar/,
    );
    await rm(sidecar);
    await writeFile(sidecar, "changed", { mode: 0o600 });
    await assert.rejects(
      validateExistingStagingHomes({ machine: f.machine, home: f.home, expected }),
      /declaration changed/,
    );
    await writeFile(`${f.machine.codexHome}/unknown.sqlite-wal`, "bad", { mode: 0o600 });
    await assert.rejects(
      snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
      /unknown/,
    );
  } finally {
    await rm(f.parent, { recursive: true });
  }
});

test("forward file identity and exact private-home declarations reject replacement and unapproved inputs", async () => {
  const f = await fixture();
  try {
    const path = `${f.root}/agent-communication/projects/${project}/e1.sqlite`;
    // Retain the original inode while allocating its replacement, preventing inode reuse.
    const replacement = `${f.parent}/replacement`;
    await writeFile(replacement, "replacement", { mode: 0o600 });
    const { rename } = await import("node:fs/promises");
    await rename(path, `${path}.retained`);
    await rename(replacement, path);
    await assert.rejects(assertStoppedFileIdentities(f.declaration), /replaced or moved/);
    await mkdir(`${f.home}/.cache`, { mode: 0o700 });
    await assert.rejects(
      validateStagingUserHome({ machine: f.machine, home: f.home }),
      /fresh staging HOME/,
    );
    for (const [directory, name] of [
      [f.home, "unknown"],
      [`${f.home}/.cache`, "auth.json"],
      [f.machine.codexHome, "sessions"],
    ]) {
      assert.ok(directory !== undefined && name !== undefined);
      const file = `${directory}/${name}`;
      await writeFile(file, "unapproved", { mode: 0o600 });
      await assert.rejects(
        snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
        /unknown|authentication import/,
      );
      await rm(file);
    }
    await symlink(`${f.parent}/outside`, `${f.home}/.cache/escape`);
    await writeFile(`${f.parent}/outside`, "outside", { mode: 0o600 });
    await assert.rejects(
      snapshotExistingStagingHomes({ machine: f.machine, home: f.home }),
      /symlink escapes/,
    );
    await rm(`${f.home}/.cache/escape`);
    await writeFile(`${f.machine.codexHome}/state_5.sqlite`, "changed state");
    await assert.rejects(
      validateExistingStagingHomes({
        machine: f.machine,
        home: f.home,
        expected: f.declaration.homes,
      }),
      /declaration changed/,
    );
  } finally {
    await rm(f.parent, { recursive: true });
  }
});

async function reference(path: string, value: unknown) {
  await writeFile(path, typeof value === "string" ? value : encode(value), { mode: 0o600 });
  return { path, sha256: await fingerprint(path) };
}

async function fixtureHomes(
  machine: Awaited<ReturnType<typeof fixture>>["machine"],
  home: string,
  parent: string,
): Promise<void> {
  await prepareStagingUserHome({ machine, home });
  await mkdir(machine.codexHome, { mode: 0o700 });
  const trust = `[projects.${JSON.stringify(parent)}]\ntrust_level = "trusted"\n`;
  await reference(`${machine.codexHome}/config.toml`, trust);
  await reference(
    `${machine.codexHome}/codex-router-debug.config.toml`,
    `model_provider = "codex-router-debug"\n${trust}\n[model_providers.codex-router-debug]\nname = "OpenAI"\nbase_url = "http://127.0.0.1:18827/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = true\n\n[features]\nenable_request_compression = false\n`,
  );
  await reference(`${machine.codexHome}/state_5.sqlite`, "synthetic already-created native state");
}

async function fixtureStops(root: string, routerPath: string) {
  const ledger: CommandLedgerEntry[] = [];
  const observations = [];
  for (const [role, pid] of [
    ["router", 900001],
    ["host", 900002],
  ] as const) {
    const commandId = commandIds[role],
      exit = role === "router" ? null : 0;
    ledger.push(
      {
        kind: "started",
        at,
        commandId,
        mode: "daemon",
        executable: routerPath,
        arguments:
          role === "router"
            ? ["serve", "--state-db", `${root}/state.sqlite`]
            : ["host", "--router-root", root],
        cwd: root,
        environmentKeys: [],
      },
      { kind: "spawned", at, commandId, pid },
      {
        kind: "ended",
        at: settled,
        commandId,
        pid,
        exit,
        signal: role === "router" ? "SIGTERM" : null,
        error: null,
      },
    );
    observations.push({
      kind: "processStopped",
      at: settled,
      value: { pid: BigInt(pid), exit: exit === null ? null : BigInt(exit) },
    });
  }
  ledger.push({ kind: "baseline", at, baselineId: marker });
  await reference(
    `${root}/verification-commands.jsonl`,
    ledger.map((row) => encode(row)).join("\n") + "\n",
  );
  return { ledger, observations };
}

async function fixtureMachine(parent: string, root: string) {
  const artifact = async (name: string) => ({
    ...(await reference(`${parent}/${name}`, "synthetic executable bytes")),
    identifier: `dev.shravansunder.${name}.debug`,
    team: "974QD84WVC",
  });
  const router = await artifact("codex-router"),
    cli = await artifact("agent-collaboration");
  const machine = {
    kind: "local",
    name: "sunbook",
    root,
    temporaryParent: parent,
    nativeCwd: parent,
    codexHome: `${parent}/codex-home`,
    port: 18827,
    router,
    cli,
    enrollment: { kind: "unattached" },
  } as const;
  return machine;
}

test("retired upgrade resume refuses before declared-root inspection", async (): Promise<void> => {
  await assert.rejects(
    Installation.resumeStoppedUpgrade({}),
    /retired: the updater was replaced by scripts\/one-off\/2026-11-convert-router-root\.ts/,
  );
});
