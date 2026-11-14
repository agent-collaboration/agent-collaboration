import assert from "node:assert/strict";
import { test } from "node:test";
import { cp, mkdir, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises";
import { createFixtureDirectory } from "../../verification/test-support/fixture-directory.ts";
import { join } from "node:path";
import { createServer } from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { DatabaseSync } from "node:sqlite";
import { convertRouterRoot, rootInventory } from "./2026-11-convert-router-root.ts";

const script = new URL("./2026-11-convert-router-root.ts", import.meta.url).pathname;
const fixture = new URL("./fixtures/released-0.1.125", import.meta.url).pathname;
const providers =
  '{"version":1,"providers":{"claude":{"enabled":false,"executable":null,"arguments":[]},"cursor":{"enabled":false,"executable":null,"arguments":[]}}}';
const ports = '{"router":18827,"mcp":18828,"tailscaleHttps":443}';
async function copiedRoot(): Promise<{ parent: string; root: string; backup: string }> {
  const parent = await realpath(createFixtureDirectory("root-conversion"));
  const root = join(parent, "router"),
    backup = join(parent, "router.backup-2026-11-13");
  await cp(fixture, root, { recursive: true });
  await writeFile(join(root, "providers.json"), providers);
  await writeFile(join(root, "ports.json"), ports);
  await writeFile(join(root, "unknown-future-file"), "backup-only unknown bytes");
  await writeFile(join(root, "routers.jsonc"), '// machine list\n{"machines": []}\n');
  await writeFile(join(root, "agent-sessions.jsonc"), '{"version":1,"routers":[]}');
  await writeFile(join(root, "settings.jsonc"), '{"ports":{"router":18827}}');
  await mkdir(join(root, "fleet"));
  await writeFile(join(root, "fleet", "enrollment.json"), "old enrollment");
  await mkdir(join(root, "activation"));
  await writeFile(join(root, "activation", "old-marker"), "old activation");
  await mkdir(join(root, "state.sqlite.credential-locks"));
  await writeFile(join(root, "state.sqlite.credential-locks", "account.lock"), "lock bytes");
  await mkdir(join(root, "agent-communication", "projects"), { recursive: true });
  for (const name of [
    "fleet-cache.sqlite",
    "fleet-outbox.sqlite",
    "board-cursors.sqlite",
    "fleet.sqlite",
    "project-board.sqlite",
    "enrollment-reset.json",
    "approval-routes.json",
  ])
    await writeFile(join(root, "agent-communication", name), `backup-only ${name}`);
  await writeFile(
    join(root, "agent-communication", "projects", "old-project.sqlite"),
    "old project",
  );
  await writeFile(
    join(root, "agent-communication", "interaction.sqlite"),
    "interaction bytes unchanged",
  );
  await writeFile(
    join(root, "agent-communication", "service-identity.json"),
    "identity bytes unchanged",
  );
  await mkdir(join(root, "snapshot-install-attempts"));
  await writeFile(join(root, "snapshot-install-attempts", "attempt.json"), "old attempt");
  await mkdir(join(root, "secrets"), { mode: 0o700 });
  await writeFile(join(root, "secrets", "fixture-secret"), "do not print this fixture marker", {
    mode: 0o600,
  });
  for (const name of [
    "agent-proxy-ui.sqlite",
    "agent-proxy-ui.sqlite-wal",
    "agent-proxy-ui.sqlite-shm",
  ])
    await writeFile(join(root, name), `quota preferences ${name}`, { mode: 0o600 });
  return { parent, root, backup };
}
function accountRows(path: string): unknown {
  const database = new DatabaseSync(path, { readOnly: true });
  try {
    return database
      .prepare("SELECT account_id,label,provider,status,active_credential_generation FROM accounts")
      .all();
  } finally {
    database.close();
  }
}

function allStoredRows(path: string): unknown {
  const database = new DatabaseSync(path, { readOnly: true });
  try {
    const names = database
      .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
      .all();
    return names.map((row) => {
      if (typeof row.name !== "string" || !/^[a-z_]+$/.test(row.name))
        throw new Error("unexpected fixture table");
      return [row.name, database.prepare('SELECT * FROM "' + row.name + '"').all()];
    });
  } finally {
    database.close();
  }
}

test("released writer fixture converts with verified backup, WAL rows and current filenames", async (): Promise<void> => {
  const input = await copiedRoot();
  try {
    const expectedRows = allStoredRows(join(input.root, "state.sqlite"));
    const before = await rootInventory(input.root);
    const plan = await convertRouterRoot({ ...input, dryRun: false });
    assert.ok(
      plan.actions.some(
        (action) => action.kind === "backupOnly" && action.source.endsWith("agent-communication"),
      ),
    );
    assert.deepEqual(allStoredRows(join(input.root, "provider-proxy.sqlite")), expectedRows);
    assert.deepEqual(await rootInventory(input.backup), before);
    assert.deepEqual(
      accountRows(join(input.root, "provider-proxy.sqlite")),
      [
        {
          account_id: "019f0000-0000-7000-8000-00000000e501",
          label: "Conversion fixture",
          provider: "openai",
          status: "disabled",
          active_credential_generation: 3,
        },
      ].map((row) => Object.assign(Object.create(null), row)),
    );
    assert.equal(
      await readFile(
        join(input.root, "provider-proxy.sqlite.credential-locks", "account.lock"),
        "utf8",
      ),
      "lock bytes",
    );
    assert.deepEqual(
      (await readdir(input.root))
        .filter((name) => !name.endsWith("-wal") && !name.endsWith("-shm"))
        .sort(),
      [
        "agent-proxy-ui.sqlite",
        "provider-proxy.sqlite",
        "provider-proxy.sqlite.credential-locks",
        "secrets",
      ],
    );
    const carriedNames = new Set([
      "secrets",
      "agent-proxy-ui.sqlite",
      "agent-proxy-ui.sqlite-wal",
      "agent-proxy-ui.sqlite-shm",
    ]);
    assert.deepEqual(
      (await rootInventory(input.root)).filter((entry) =>
        carriedNames.has(entry.relative.split("/")[0] ?? ""),
      ),
      before.filter((entry) => carriedNames.has(entry.relative.split("/")[0] ?? "")),
      "credentials and quota preferences retain every byte and permission bit",
    );
    assert.equal(await readFile(join(input.backup, "providers.json"), "utf8"), providers);
    assert.equal(await readFile(join(input.backup, "ports.json"), "utf8"), ports);
    assert.equal(
      await readFile(join(input.backup, "agent-communication", "interaction.sqlite"), "utf8"),
      "interaction bytes unchanged",
    );
    assert.equal(
      await readFile(join(input.backup, "secrets", "fixture-secret"), "utf8"),
      "do not print this fixture marker",
    );
  } finally {
    await rm(input.parent, { recursive: true });
  }
});

test("dry run plans backup-only routes without creating backup or changing any bytes", async (): Promise<void> => {
  const input = await copiedRoot();
  try {
    const before = await rootInventory(input.root);
    const result = spawnSync(process.execPath, [script, "--root", input.root, "--dry-run"], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(
      result.stdout,
      /The provider proxy \(accounts, credentials, quota preferences\) carries over/,
    );
    assert.ok(!result.stdout.includes("do not print this fixture marker"));
    assert.deepEqual(await rootInventory(input.root), before);
    assert.deepEqual(await readdir(input.parent), ["router"]);
  } finally {
    await rm(input.parent, { recursive: true });
  }
});

for (const [name, file, value] of [
  ["proxy collision", "provider-proxy.sqlite", "conflicting store"],
  ["proxy sidecar collision", "provider-proxy.sqlite-wal", "conflicting log"],
  ["credential-lock collision", "provider-proxy.sqlite.credential-locks", "conflicting locks"],
])
  test(`${name} refuses in preflight with live-root byte equality`, async (): Promise<void> => {
    const input = await copiedRoot();
    try {
      if (file === undefined || value === undefined) throw new Error("fixture tuple incomplete");
      await writeFile(join(input.root, file), value);
      const before = await rootInventory(input.root);
      await assert.rejects(
        convertRouterRoot({ ...input, dryRun: false }),
        new RegExp(file.replaceAll(".", "\\.")),
      );
      assert.deepEqual(await rootInventory(input.root), before);
      assert.deepEqual(await readdir(input.parent), ["router"]);
    } finally {
      await rm(input.parent, { recursive: true });
    }
  });

test("a live Host socket refuses without touching stores or sibling files", async (): Promise<void> => {
  const input = await copiedRoot();
  const listener = createServer();
  const socket = join(input.root, "host.sock");
  try {
    await new Promise<void>((accept, reject) => {
      listener.once("error", reject);
      listener.listen(socket, accept);
    });
    const bytes = await readFile(join(input.root, "state.sqlite-wal"));
    await assert.rejects(convertRouterRoot({ ...input, dryRun: false }), /host\.sock/);
    assert.deepEqual(await readFile(join(input.root, "state.sqlite-wal")), bytes);
    assert.deepEqual(await readdir(input.parent), ["router"]);
  } finally {
    await new Promise<void>((accept) => listener.close(() => accept()));
    await rm(input.parent, { recursive: true });
  }
});

test("a busy WAL checkpoint is an apply failure with verified backup and exact restore command", async (): Promise<void> => {
  const input = await copiedRoot();
  const reader = new DatabaseSync(join(input.root, "state.sqlite"));
  let readerOpen = true;
  try {
    reader.exec("BEGIN");
    reader.prepare("SELECT * FROM accounts").all();
    const writer = new DatabaseSync(join(input.root, "state.sqlite"));
    writer.exec("CREATE TABLE checkpoint_hold_fixture(value TEXT)");
    writer.close();
    const before = await rootInventory(input.root);
    let restoreCommand = "";
    await assert.rejects(convertRouterRoot({ ...input, dryRun: false }), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.match(error.message, /Apply failed at proxy.*checkpoint is busy/);
      restoreCommand = error.message.split("Restore while Host is stopped: ")[1] ?? "";
      assert.match(restoreCommand, /^mv .* && cp -a /);
      return true;
    });
    assert.deepEqual(await rootInventory(input.backup), before);
    assert.ok(!(await readdir(input.root)).includes("provider-proxy.sqlite"));
    reader.exec("ROLLBACK");
    reader.close();
    readerOpen = false;
    const restored = spawnSync("/bin/sh", ["-c", restoreCommand], { encoding: "utf8" });
    assert.equal(restored.status, 0, restored.stderr);
    assert.deepEqual(await rootInventory(input.root), before);
    assert.ok((await readdir(input.parent)).some((name) => name.startsWith("router.failed-")));
  } finally {
    if (readerOpen) {
      reader.exec("ROLLBACK");
      reader.close();
    }
    await rm(input.parent, { recursive: true });
  }
});

test("an open Host singleton artifact refuses before socket publication", async (): Promise<void> => {
  const input = await copiedRoot();
  const lock = join(input.root, "host.lock");
  await writeFile(lock, "");
  const child = spawn(
    process.execPath,
    [
      "-e",
      "const f=require('node:fs'); const fd=f.openSync(process.argv[1],'r'); process.stdout.write('ready'); process.stdin.resume(); process.stdin.on('end',()=>f.closeSync(fd));",
      lock,
    ],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  try {
    await once(child.stdout, "data");
    const before = await rootInventory(input.root);
    await assert.rejects(convertRouterRoot({ ...input, dryRun: false }), /host\.lock/);
    assert.deepEqual(await rootInventory(input.root), before);
    assert.deepEqual(await readdir(input.parent), ["router"]);
  } finally {
    child.stdin.end();
    await once(child, "exit");
    await rm(input.parent, { recursive: true });
  }
});

test("existing backup is never overwritten", async (): Promise<void> => {
  const input = await copiedRoot();
  try {
    await mkdir(input.backup);
    await writeFile(join(input.backup, "keep"), "existing bytes");
    const before = await rootInventory(input.root);
    await assert.rejects(convertRouterRoot({ ...input, dryRun: false }), /backup already exists/);
    assert.deepEqual(await rootInventory(input.root), before);
    assert.equal(await readFile(join(input.backup, "keep"), "utf8"), "existing bytes");
  } finally {
    await rm(input.parent, { recursive: true });
  }
});

test("orphan old WAL refuses without creating a database", async (): Promise<void> => {
  const input = await copiedRoot();
  try {
    await rm(join(input.root, "state.sqlite"));
    const before = await rootInventory(input.root);
    await assert.rejects(convertRouterRoot({ ...input, dryRun: false }), /orphan SQLite sidecars/);
    assert.deepEqual(await rootInventory(input.root), before);
  } finally {
    await rm(input.parent, { recursive: true });
  }
});
