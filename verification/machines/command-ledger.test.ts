import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, chmod, appendFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUIDv7 } from "node:crypto";
import { command, OwnedProcess } from "./owned-process.ts";
import { Receipt } from "../receipt.ts";
import {
  registerCommandRoot,
  readCommandLedger,
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
