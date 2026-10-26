// Real-PTY checks of PtyProcess against harmless inline Node children, and of the
// installed-Codex entry's refusals using Node as the wrong target. Each child reports
// what it observed from inside the terminal; expected values are literals.
import { strict as assert } from "node:assert";
import { spawn as spawnProcess } from "node:child_process";
import { mkdtemp, readFile, readdir, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test, type TestContext } from "node:test";
import { z } from "zod";
import { PtyProcess, type PtyLaunch, type PtyOutputWait } from "./pty-process.ts";
import { hasCodexPreamble } from "./codex-pty.ts";

const failureDeadline = 10_000;
const absenceDeadline = 300;

test("Codex diagnostics may precede its own complete version or help line", () => {
  const warning = "WARNING: proceeding, even though we could not create PATH aliases\r\n";
  assert.equal(hasCodexPreamble("--version", `${warning}codex-cli 0.163.0\r\n`), true);
  assert.equal(hasCodexPreamble("--help", `${warning}Codex CLI\r\n`), true);
  assert.equal(hasCodexPreamble("--version", "v26.11.1\r\n"), false);
  assert.equal(hasCodexPreamble("--help", "Usage: node [options]\r\n"), false);
  assert.equal(hasCodexPreamble("--help", "the text Codex CLI appears mid-line\r\n"), false);
});

function nodeChildLaunch(source: string, outputLimitBytes = 64 * 1024): PtyLaunch {
  return {
    executable: process.execPath,
    arguments: ["-e", source],
    cwd: tmpdir(),
    environment: { PATH: "/usr/bin:/bin", TERM: "xterm-256color" },
    columns: 80,
    rows: 24,
    outputLimitBytes,
  };
}

/** Starts a child whose cleanup is registered before any assertion can fail. */
function startNodeChild(
  context: TestContext,
  source: string,
  outputLimitBytes?: number,
): PtyProcess {
  const child = PtyProcess.start(nodeChildLaunch(source, outputLimitBytes));
  context.after(async (): Promise<void> => {
    await child.stop(failureDeadline);
  });
  return child;
}

function matchedOffset(wait: PtyOutputWait): number {
  assert.equal(wait.kind, "matched", `expected a match, got ${JSON.stringify(wait)}`);
  return wait.kind === "matched" ? wait.offset : -1;
}

const ttyReportSchema = z.object({
  stdin: z.boolean(),
  stdout: z.boolean(),
  stderr: z.boolean(),
  columns: z.number(),
  rows: z.number(),
});

test("the child's stdin, stdout and stderr are all a terminal of the requested size", async (t) => {
  const child = startNodeChild(
    t,
    `
    const report = { stdin: process.stdin.isTTY === true, stdout: process.stdout.isTTY === true,
      stderr: process.stderr.isTTY === true, columns: process.stdout.columns, rows: process.stdout.rows };
    process.stdout.write("REPORT " + JSON.stringify(report) + " END\\n");`,
  );
  const end = matchedOffset(await child.waitForOutput(" END", 0, failureDeadline));
  const start = matchedOffset(await child.waitForOutput("REPORT ", 0, failureDeadline));
  const report = ttyReportSchema.parse(JSON.parse(child.output.slice(start + 7, end)));
  assert.deepEqual(report, { stdin: true, stdout: true, stderr: true, columns: 80, rows: 24 });
  assert.deepEqual(await child.waitForExit(failureDeadline), {
    kind: "exited",
    exit: { exitCode: 0, signal: 0 },
  });
});

test("keyboard input reaches the child, which answers with text echo cannot produce", async (t) => {
  const child = startNodeChild(
    t,
    `
    process.stdin.setRawMode(true);
    process.stdout.write("READY\\n");
    process.stdin.on("data", (bytes) => {
      process.stdout.write("REPLY:" + [...bytes.toString()].reverse().join("").toUpperCase() + "\\n");
    });`,
  );
  matchedOffset(await child.waitForOutput("READY", 0, failureDeadline));
  const beforeInput = child.output.length;
  child.write("abc");
  const reply = matchedOffset(await child.waitForOutput("REPLY:CBA", beforeInput, failureDeadline));
  assert.ok(reply >= beforeInput);
  assert.deepEqual(child.inputs, [{ outputOffset: beforeInput, data: "abc" }]);
  assert.equal((await child.stop(failureDeadline)).signal, 15);
});

test("an echo of the input alone is not accepted as the child's reply", async (t) => {
  // Cooked mode: the terminal echoes "abc", but the child never answers.
  const child = startNodeChild(t, `process.stdout.write("READY\\n"); process.stdin.resume();`);
  matchedOffset(await child.waitForOutput("READY", 0, failureDeadline));
  const beforeInput = child.output.length;
  child.write("abc");
  matchedOffset(await child.waitForOutput("abc", beforeInput, failureDeadline));
  assert.deepEqual(await child.waitForOutput("REPLY:CBA", beforeInput, absenceDeadline), {
    kind: "deadline",
  });
});

test("a resize is reported by the child after the resize, not before", async (t) => {
  const child = startNodeChild(
    t,
    `
    const report = () => process.stdout.write("SIZE:" + process.stdout.columns + "x" + process.stdout.rows + "\\n");
    process.stdout.on("resize", report);
    report();
    process.stdin.resume();`,
  );
  matchedOffset(await child.waitForOutput("SIZE:80x24", 0, failureDeadline));
  const beforeResize = child.output.length;
  child.resize(100, 30);
  matchedOffset(await child.waitForOutput("SIZE:100x30", beforeResize, failureDeadline));
});

test("a normal exit is reported and later input or resize is refused", async (t) => {
  const child = startNodeChild(t, `process.stdout.write("BYE\\n"); process.exitCode = 7;`);
  assert.deepEqual(await child.waitForExit(failureDeadline), {
    kind: "exited",
    exit: { exitCode: 7, signal: 0 },
  });
  assert.ok(child.output.includes("BYE"));
  assert.throws(() => child.write("late"), /exited; input refused/);
  assert.throws(() => child.resize(90, 20), /exited; resize refused/);
});

test("stale, wrong or missing output fails the observation", async (t) => {
  const stale = startNodeChild(
    t,
    `process.stdout.write("MARKER\\nREADY\\n"); process.stdin.resume();`,
  );
  matchedOffset(await stale.waitForOutput("READY", 0, failureDeadline));
  const afterReady = stale.output.length;
  assert.deepEqual(await stale.waitForOutput("MARKER", afterReady, absenceDeadline), {
    kind: "deadline",
  });
  const wrong = startNodeChild(t, `process.stdout.write("OTHER\\n"); process.stdin.resume();`);
  matchedOffset(await wrong.waitForOutput("OTHER", 0, failureDeadline));
  assert.deepEqual(await wrong.waitForOutput("EXPECTED", 0, absenceDeadline), {
    kind: "deadline",
  });
  const silent = startNodeChild(t, `process.stdin.resume();`);
  assert.deepEqual(await silent.waitForOutput("ANY", 0, absenceDeadline), { kind: "deadline" });
});

const floodAfterEarlyMarker = `
  process.stdout.write("EARLY\\n");
  const chunk = "x".repeat(65536);
  const flood = () => { while (process.stdout.write(chunk)) {} process.stdout.once("drain", flood); };
  flood();`;

test("output beyond the bound fails every observation and ends the child", async (t) => {
  const child = startNodeChild(t, floodAfterEarlyMarker, 64 * 1024);
  assert.equal((await child.waitForExit(failureDeadline)).kind, "exited");
  assert.ok(child.overflowed);
  assert.ok(Buffer.byteLength(child.output) <= 64 * 1024);
  // The marker arrived before the overflow, but an incomplete capture decides nothing.
  assert.ok(child.output.includes("EARLY"));
  assert.deepEqual(await child.waitForOutput("EARLY", 0, failureDeadline), { kind: "overflowed" });
});

test("stop ends only its own child, escalating when SIGTERM is ignored", async (t) => {
  const child = startNodeChild(
    t,
    `
    process.on("SIGTERM", () => process.stdout.write("IGNORED SIGTERM\\n"));
    process.stdout.write("RUNNING\\n");
    setInterval(() => {}, 60_000);`,
  );
  matchedOffset(await child.waitForOutput("RUNNING", 0, failureDeadline));
  const exit = await child.stop(1_000);
  assert.equal(exit.signal, 9);
  assert.ok(child.output.includes("IGNORED SIGTERM"));
  assert.throws(() => process.kill(child.pid, 0), { code: "ESRCH" });
});

test("invalid launches are refused before anything is spawned", () => {
  const valid = nodeChildLaunch(`process.exit(0);`);
  for (const invalid of [
    { ...valid, executable: "" },
    { ...valid, columns: 0 },
    { ...valid, rows: 2.5 },
    { ...valid, outputLimitBytes: 0 },
    { ...valid, outputLimitBytes: -1 },
  ])
    assert.throws(() => PtyProcess.start(invalid), z.ZodError);
});

test("empty markers, out-of-range offsets, bad deadlines and sizes are refused", async (t) => {
  const child = startNodeChild(t, `process.stdout.write("READY\\n"); process.stdin.resume();`);
  matchedOffset(await child.waitForOutput("READY", 0, failureDeadline));
  const end = child.output.length;
  const invalidWaits: ReadonlyArray<readonly [string, number, number]> = [
    ["", 0, 100],
    ["READY", -1, 100],
    ["READY", 0.5, 100],
    ["READY", end + 1, 100],
    ["READY", 0, 0],
    ["READY", 0, 1.5],
  ];
  for (const [marker, offset, deadline] of invalidWaits)
    assert.throws(() => child.waitForOutput(marker, offset, deadline), z.ZodError);
  assert.throws(() => child.resize(0, 10), z.ZodError);
  assert.throws(() => child.resize(10.5, 10), z.ZodError);
});

const codexEntry = new URL("./codex-pty.ts", import.meta.url).pathname;

/** Runs the Codex entry against `codex` with a fresh, test-owned evidence directory. */
async function runCodexEntry(
  context: TestContext,
  codex: string,
  environment: Readonly<Record<string, string>>,
): Promise<{
  readonly exitCode: number | null;
  readonly evidence: string;
  readonly stderr: string;
}> {
  const evidence = await realpath(await mkdtemp(join(tmpdir(), "codex-pty-check-")));
  context.after(async (): Promise<void> => {
    await rm(evidence, { recursive: true, force: true });
  });
  const entry = spawnProcess(
    process.execPath,
    [codexEntry, "--codex", codex, "--evidence", evidence],
    { env: { ...environment }, stdio: ["ignore", "ignore", "pipe"] },
  );
  let stderr = "";
  entry.stderr.on("data", (chunk: Buffer): void => {
    stderr += chunk.toString();
  });
  const exitCode = await new Promise<number | null>((resolve, reject): void => {
    entry.once("error", reject);
    entry.once("close", resolve);
  });
  return { exitCode, evidence, stderr };
}

const proofSchema = z.object({
  passed: z.boolean(),
  runs: z.array(
    z.object({
      arguments: z.array(z.string()),
      exitCode: z.number().nullable(),
      passed: z.boolean(),
    }),
  ),
});

test("the Codex entry rejects another program that answers --version and --help", async (t) => {
  // Node exits 0 with a version number and its own usage text, but it is not Codex.
  const { exitCode, evidence } = await runCodexEntry(t, process.execPath, {
    PATH: "/usr/bin:/bin",
  });
  assert.equal(exitCode, 1);
  const proof = proofSchema.parse(
    JSON.parse(await readFile(join(evidence, "codex-pty-proof.json"), "utf8")),
  );
  assert.equal(proof.passed, false);
  assert.deepEqual(
    proof.runs.map((run) => [run.arguments, run.exitCode, run.passed]),
    [
      [["--version"], 0, false],
      [["--help"], 0, false],
    ],
  );
});

test("the Codex entry refuses a different CODEX_HOME before spawning anything", async (t) => {
  const other = await realpath(await mkdtemp(join(tmpdir(), "codex-pty-other-home-")));
  t.after(async (): Promise<void> => {
    await rm(other, { recursive: true, force: true });
  });
  const { exitCode, evidence, stderr } = await runCodexEntry(t, process.execPath, {
    PATH: "/usr/bin:/bin",
    CODEX_HOME: other,
  });
  assert.equal(exitCode, 1);
  assert.match(stderr, /CODEX_HOME names a different Codex home; refused before any spawn/);
  assert.deepEqual(await readdir(evidence), []);
});
