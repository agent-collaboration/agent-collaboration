// Model-free proof that the installed Codex client starts under a real OS
// pseudoterminal: it runs only `--version` and `--help` and keeps the exact raw output,
// exit and executable hash. It proves the real PTY and client entry only, not the
// Router, a provider, a model, a session or any menu.
import { realpath, stat, writeFile } from "node:fs/promises";
import { userInfo } from "node:os";
import { isAbsolute, join } from "node:path";
import { parseArgs } from "node:util";
import { createHash } from "node:crypto";
import { z } from "zod";
import { fingerprint, privateDirectory } from "../machines/owned-process.ts";
import { PtyProcess } from "./pty-process.ts";

const usage = `Usage: node verification/terminal/codex-pty.ts --codex <absolute codex executable> --evidence <directory>

From the repository root, install verification dependencies first. On macOS run:
  pnpm --dir verification run build:pty
The permanent PTY checks run with:
  pnpm --dir verification run test:terminal

Runs the installed Codex client's --version and --help, each once, under a real OS PTY.
It uses the OS account's own home (never an inherited HOME) and its default Codex home,
~/.codex, with no override: a CODEX_HOME naming any other directory is refused before
anything is spawned. It fails if config.toml or codex-router-debug.config.toml there
changes across the runs.
--evidence must be an existing, canonical, owner-private (0700) directory; the raw output
of each run and codex-pty-proof.json go there. Exits 1 if any check fails.
This proves the real PTY/client entry only: no Router, provider, model, session or menu.`;

const optionsSchema = z.object({
  codex: z.string().refine(isAbsolute, "--codex must be an absolute path"),
  evidence: z.string().refine(isAbsolute, "--evidence must be an absolute path"),
});

const runDeadline = 30_000;

/** A file's SHA-256, or "absent" when it does not exist. */
async function fingerprintIfPresent(path: string): Promise<string> {
  try {
    return await fingerprint(path);
  } catch (failure: unknown) {
    if (failure instanceof Error && "code" in failure && failure.code === "ENOENT") return "absent";
    throw failure;
  }
}

interface CodexRun {
  readonly arguments: readonly string[];
  readonly outcome: "exited" | "deadline";
  readonly exitCode: number | null;
  readonly signal: number | null;
  readonly overflowed: boolean;
  readonly outputFile: string;
  readonly outputBytes: number;
  readonly outputSha256: string;
  readonly passed: boolean;
}

async function runCodex(
  executable: string,
  evidence: string,
  environment: Readonly<Record<string, string>>,
  argument: "--version" | "--help",
): Promise<CodexRun> {
  const child = PtyProcess.start({
    executable,
    arguments: [argument],
    cwd: evidence,
    environment,
    columns: 120,
    rows: 40,
    outputLimitBytes: 1024 * 1024,
  });
  const ending = await child.waitForExit(runDeadline);
  if (ending.kind === "deadline") await child.stop(runDeadline);
  const outputFile = join(evidence, `codex${argument}.pty-output`);
  await writeFile(outputFile, child.output, { flag: "wx", mode: 0o600 });
  const exit = ending.kind === "exited" ? ending.exit : undefined;
  return {
    arguments: [argument],
    outcome: ending.kind,
    exitCode: exit?.exitCode ?? null,
    signal: exit?.signal ?? null,
    overflowed: child.overflowed,
    outputFile,
    outputBytes: Buffer.byteLength(child.output),
    outputSha256: createHash("sha256").update(child.output).digest("hex"),
    passed: exit?.exitCode === 0 && !child.overflowed && hasCodexPreamble(argument, child.output),
  };
}

/** Codex may emit diagnostics before its own complete version or help line. */
export function hasCodexPreamble(argument: "--version" | "--help", output: string): boolean {
  return (argument === "--version" ? /^codex-cli \d+\.\d+\.\d+\r?$/m : /^Codex CLI\r?$/m).test(
    output,
  );
}

/**
 * The OS account's home (from the user database, not HOME) and its default Codex home.
 * A CODEX_HOME that names any other directory is refused, so no run uses another home.
 */
async function normalCodexHome(): Promise<{ readonly home: string; readonly codexHome: string }> {
  const home = userInfo().homedir;
  const defaultCodexHome = join(home, ".codex");
  const codexHome = await realpath(defaultCodexHome).catch((failure: unknown): string => {
    // A machine without Codex state still has this normal default path. Do not
    // create it or invent another home just to run the model-free entry.
    if (failure instanceof Error && "code" in failure && failure.code === "ENOENT")
      return defaultCodexHome;
    throw failure;
  });
  const configured = process.env["CODEX_HOME"];
  if (configured !== undefined) {
    const configuredReal = await realpath(configured).catch((): string => configured);
    if (configuredReal !== codexHome)
      throw new Error("CODEX_HOME names a different Codex home; refused before any spawn");
  }
  return { home, codexHome };
}

async function main(): Promise<number> {
  const parsed = parseArgs({
    options: { codex: { type: "string" }, evidence: { type: "string" }, help: { type: "boolean" } },
    strict: true,
  });
  if (parsed.values.help === true) {
    process.stdout.write(`${usage}\n`);
    return 0;
  }
  const options = optionsSchema.parse(parsed.values);
  await privateDirectory(options.evidence);
  const executable = await realpath(options.codex);
  if (((await stat(executable)).mode & 0o111) === 0) throw new Error("--codex is not executable");
  const { home, codexHome } = await normalCodexHome();
  // The child finds its normal Codex home from HOME; no CODEX_HOME is passed.
  const environment = { PATH: "/usr/bin:/bin", TERM: "xterm-256color", HOME: home };
  const settingsFiles = [
    join(codexHome, "config.toml"),
    join(codexHome, "codex-router-debug.config.toml"),
  ];
  const settingsBefore = await Promise.all(settingsFiles.map(fingerprintIfPresent));
  const runs = [
    await runCodex(executable, options.evidence, environment, "--version"),
    await runCodex(executable, options.evidence, environment, "--help"),
  ];
  const settingsAfter = await Promise.all(settingsFiles.map(fingerprintIfPresent));
  const settingsUnchanged = settingsFiles.every(
    (_file, index) => settingsBefore[index] === settingsAfter[index],
  );
  const passed = settingsUnchanged && runs.every((run) => run.passed);
  const proof = {
    codex: options.codex,
    resolvedExecutable: executable,
    executableSha256: await fingerprint(executable),
    node: process.version,
    environment,
    runs,
    settings: {
      files: settingsFiles,
      before: settingsBefore,
      after: settingsAfter,
      unchanged: settingsUnchanged,
    },
    passed,
    limits: [
      "Proves only that the installed client starts and answers --version/--help under a real OS PTY.",
      "No Router, provider, model, session or menu behaviour is observed; only the named settings files are checked for change.",
      "Raw PTY output is not a rendered screen.",
    ],
  };
  await writeFile(
    join(options.evidence, "codex-pty-proof.json"),
    `${JSON.stringify(proof, null, 2)}\n`,
    {
      flag: "wx",
      mode: 0o600,
    },
  );
  process.stdout.write(`${JSON.stringify({ passed, settingsUnchanged, runs })}\n`);
  return passed ? 0 : 1;
}

if (import.meta.main) process.exitCode = await main();
