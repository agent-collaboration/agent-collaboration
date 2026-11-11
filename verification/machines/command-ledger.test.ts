import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readFile,
  rm,
  chmod,
  appendFile,
  truncate,
  writeFile,
  open,
  stat,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUIDv7 } from "node:crypto";
import { command, OwnedProcess } from "./owned-process.ts";
import { Receipt } from "../receipt.ts";
import {
  registerCommandRoot,
  readCommandLedger,
  readCommandLedgerEntries,
  markCommandBaseline,
  enrollInvocationsSince,
} from "./command-ledger.ts";

test("registered root records actual argv/PID/outcomes and enrollment calls without environment values", async (): Promise<void> => {
  const root = await mkdtemp(join(tmpdir(), "2026-11-command-ledger-"));
  const canonical = await import("node:fs/promises").then((fs) => fs.realpath(root));
  const release = await registerCommandRoot(canonical);
  try {
    const baseline = await markCommandBaseline(canonical);
    const observed = await command(
      process.execPath,
      ["-e", "process.exit(0)", "--", "--router-root", canonical],
      {
        environment: { ...process.env, VERIFICATION_PRIVATE_TEST: "never-record-this-value" },
        cwd: canonical,
      },
    );
    assert.equal(observed.exit, 0);
    assert.equal(await enrollInvocationsSince(canonical, baseline), 0);
    // A real process with the enrollment argv exercises recording/classification, not an enrollment server.
    await command("/usr/bin/printf", ["%s", "fleet", "enroll", "--router-root", canonical], {
      cwd: canonical,
    });
    assert.equal(await enrollInvocationsSince(canonical, baseline), 1);
    const rows = await readCommandLedger(canonical);
    assert.equal(rows.filter((row) => row.kind === "spawned").length, 2);
    assert.equal(rows.filter((row) => row.kind === "ended").length, 2);
    const start = rows.find((row) => row.kind === "started");
    assert.ok(start?.kind === "started");
    assert.ok(start.environmentKeys.includes("VERIFICATION_PRIVATE_TEST"));
    assert.equal(start.cwd, canonical);
    assert.equal(
      (await readFile(`${canonical}/verification-commands.jsonl`, "utf8")).includes(
        "never-record-this-value",
      ),
      false,
    );
    await appendFile(
      `${canonical}/verification-commands.jsonl`,
      `${JSON.stringify({ kind: "started", at: new Date().toISOString(), commandId: randomUUIDv7(), mode: "command", executable: "/fixture", arguments: [], cwd: canonical, environmentKeys: [] })}\n`,
    );
    await assert.rejects(enrollInvocationsSince(canonical, baseline), /outcome is missing/);
  } finally {
    release();
    await rm(root, { recursive: true });
  }
});
test("ledger refuses public permissions and truncated rows", async (): Promise<void> => {
  const root = await mkdtemp(join(tmpdir(), "2026-11-command-ledger-"));
  const canonical = await import("node:fs/promises").then((fs) => fs.realpath(root));
  const release = await registerCommandRoot(canonical);
  try {
    const path = `${canonical}/verification-commands.jsonl`;
    await chmod(path, 0o644);
    await assert.rejects(readCommandLedger(canonical), /mode0600/);
    await chmod(path, 0o600);
    await appendFile(path, "{");
    await assert.rejects(readCommandLedger(canonical), /complete/);
  } finally {
    release();
    await rm(root, { recursive: true });
  }
});

test("a ledger scan validates its frozen complete prefix and the next scan rejects a partial append", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-ledger-prefix-"));
  const root = await import("node:fs/promises").then((fs) => fs.realpath(temporary));
  const release = await registerCommandRoot(root);
  try {
    const marker = await markCommandBaseline(root);
    const scan = readCommandLedgerEntries(root);
    const first = await scan.next();
    assert.ok(!first.done);
    assert.equal(first.value.kind, "baseline");
    await appendFile(`${root}/verification-commands.jsonl`, "{");
    assert.equal((await scan.next()).done, true);
    await assert.rejects(enrollInvocationsSince(root, marker), /complete/);
  } finally {
    release();
    await rm(temporary, { recursive: true });
  }
});

test("streaming enrollment checks still reject malformed history before the selected baseline", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-ledger-earlier-"));
  const root = await import("node:fs/promises").then((fs) => fs.realpath(temporary));
  const release = await registerCommandRoot(root);
  try {
    const marker = await markCommandBaseline(root);
    const path = `${root}/verification-commands.jsonl`;
    const original = await readFile(path, "utf8");
    await writeFile(path, `{"kind":"not-a-ledger-row"}\n${original}`, { mode: 0o600 });
    await assert.rejects(enrollInvocationsSince(root, marker));
  } finally {
    release();
    await rm(temporary, { recursive: true });
  }
});

test("registration and enrollment proof scan a complete ledger above512MiB within96MiB heap", async (): Promise<void> => {
  const root = await import("node:fs/promises").then(async (fs) =>
    fs.realpath(await mkdtemp(join(tmpdir(), "2026-11-streamed-ledger-"))),
  );
  const release = await registerCommandRoot(root);
  try {
    const marker = await markCommandBaseline(root);
    const path = `${root}/verification-commands.jsonl`;
    const at = "2026-11-11T00:00:00.000Z";
    const padding = "x".repeat(128 * 1024);
    const handle = await open(path, "a");
    try {
      for (let index = 0; index < 4102; index += 1) {
        const commandId = randomUUIDv7();
        const arguments_ = index === 0 || index === 4101 ? ["fleet", "enroll"] : [padding];
        await handle.writeFile(
          `${JSON.stringify({ kind: "started", at, commandId, mode: "command", executable: "/fixture", arguments: arguments_, cwd: root, environmentKeys: [] })}\n${JSON.stringify({ kind: "ended", at, commandId, pid: 1000 + index, exit: 0, signal: null, error: null })}\n`,
        );
      }
    } finally {
      await handle.close();
    }
    const before = await stat(path);
    assert.ok(before.size > 512 * 1024 * 1024);
    // The scanner, rather than its parent command, owns registration in the child.
    release();
    const module = new URL("./command-ledger.ts", import.meta.url).href;
    const source = `import {registerCommandRoot,enrollInvocationsSince} from ${JSON.stringify(module)};
const [root, marker] = process.argv.slice(1);
if (root === undefined || marker === undefined) throw new Error("fixture inputs");
const release = await registerCommandRoot(root);
try { console.log(await enrollInvocationsSince(root, marker)); } finally { release(); }`;
    const result = await command(
      process.execPath,
      ["--max-old-space-size=96", "--input-type=module", "-e", source, "--", root, marker],
      { timeoutSeconds: 60 },
    );
    assert.equal(result.exit, 0, result.stderr);
    assert.equal(result.stdout.trim(), "2", "both early and late enrollment rows must count");
    const after = await stat(path);
    assert.equal(after.size, before.size);
    assert.equal(after.mtimeMs, before.mtimeMs);
    await appendFile(path, "{");
    await assert.rejects(registerCommandRoot(root), /complete/);
  } finally {
    release();
    await rm(root, { recursive: true });
  }
});

test("a long-run ledger above128MiB keeps every row and its original zero-enroll marker", async () => {
  const root = await import("node:fs/promises").then(async (fs) =>
    fs.realpath(await mkdtemp(join(tmpdir(), "2026-11-long-ledger-"))),
  );
  const release = await registerCommandRoot(root);
  try {
    const marker = await markCommandBaseline(root),
      path = `${root}/verification-commands.jsonl`;
    const at = "2026-11-10T05:00:00.000Z",
      rows = [];
    for (let index = 0; index < 33000; index += 1) {
      const commandId = randomUUIDv7();
      rows.push(
        JSON.stringify({
          kind: "started",
          at,
          commandId,
          mode: "command",
          executable: "/fixture/observation",
          arguments: ["x".repeat(4096)],
          cwd: root,
          environmentKeys: [],
        }),
        JSON.stringify({
          kind: "ended",
          at,
          commandId,
          pid: 1000 + index,
          exit: 0,
          signal: null,
          error: null,
        }),
      );
    }
    await appendFile(path, rows.join("\n") + "\n");
    assert.ok((await readFile(path)).length > 128 * 1024 * 1024);
    assert.equal((await readCommandLedger(root)).length, 66001);
    assert.equal(await enrollInvocationsSince(root, marker), 0);
    const commandId = randomUUIDv7();
    await appendFile(
      path,
      [
        {
          kind: "started",
          at,
          commandId,
          mode: "command",
          executable: "/fixture",
          arguments: ["fleet", "enroll"],
          cwd: root,
          environmentKeys: [],
        },
        { kind: "ended", at, commandId, pid: 99999, exit: 0, signal: null, error: null },
      ]
        .map((row) => JSON.stringify(row))
        .join("\n") + "\n",
    );
    assert.equal(await enrollInvocationsSince(root, marker), 1);
    await appendFile(path, "{");
    await assert.rejects(readCommandLedger(root), /complete/);
    await writeFile(path, "", { mode: 0o600 });
    await truncate(path, 512 * 1024 * 1024 + 1);
    await assert.rejects(readCommandLedger(root), /bounded/);
  } finally {
    release();
    await rm(root, { recursive: true });
  }
});

test("owned daemon records real start, PID and terminal signal in its registered root", async (): Promise<void> => {
  const root = await mkdtemp(join(tmpdir(), "2026-11-command-ledger-"));
  const canonical = await import("node:fs/promises").then((fs) => fs.realpath(root));
  const release = await registerCommandRoot(canonical);
  try {
    const child = new OwnedProcess({
      executable: process.execPath,
      arguments: ["-e", "setInterval(()=>{},1000)", "--", "--router-root", canonical],
      environment: process.env,
      cwd: canonical,
      name: "owned-test",
      receipt: new Receipt(canonical),
    });
    await child.stop();
    const rows = await readCommandLedger(canonical);
    const spawn = rows.find((row) => row.kind === "spawned"),
      end = rows.find((row) => row.kind === "ended");
    assert.ok(spawn?.kind === "spawned" && end?.kind === "ended");
    assert.equal(spawn.pid, child.pid);
    assert.equal(end.pid, child.pid);
    assert.equal(end.signal, "SIGTERM");
  } finally {
    release();
    await rm(root, { recursive: true });
  }
});

test("self-hosted enrollment command is recorded once in each root it accesses", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-multi-root-ledger-"));
  const root = await import("node:fs/promises").then((fs) => fs.realpath(temporary));
  const hostRoot = `${root}/host`,
    centralRoot = `${root}/central`;
  const fs = await import("node:fs/promises");
  await fs.mkdir(hostRoot, { mode: 0o700 });
  await fs.mkdir(centralRoot, { mode: 0o700 });
  const releaseHost = await registerCommandRoot(hostRoot),
    releaseCentral = await registerCommandRoot(centralRoot);
  try {
    await command("/usr/bin/printf", [
      "%s",
      "fleet",
      "enroll",
      "--router-root",
      hostRoot,
      "--self-hosted-root",
      centralRoot,
    ]);
    const host = (await readCommandLedger(hostRoot)).find((entry) => entry.kind === "started");
    const central = (await readCommandLedger(centralRoot)).find(
      (entry) => entry.kind === "started",
    );
    assert.ok(host?.kind === "started" && central?.kind === "started");
    assert.equal(host.commandId, central.commandId);
    assert.deepEqual(host.arguments, central.arguments);
  } finally {
    releaseHost();
    releaseCentral();
    await rm(temporary, { recursive: true });
  }
});
