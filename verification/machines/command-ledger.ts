/** Per-root command evidence for explicitly registered owned verification roots. */
import { appendFileSync, openSync, closeSync } from "node:fs";
import { lstat, readFile, realpath } from "node:fs/promises";
import { randomUUIDv7 } from "node:crypto";
import { resolve } from "node:path";
import { z } from "zod";
import { decodeAuthoredJson, encode } from "../exact-json.ts";

const roots = new Map<string, { path: string; users: number }>();
// The normal long-run Sunclaw ledger reached38.7MiB. Retain all history and its
// original marker; this is a bounded full read, never rotation or truncation.
const maximumLedgerBytes = 128 * 1024 * 1024;
const base = z.object({ at: z.iso.datetime(), commandId: z.uuidv7() });
const started = base
  .extend({
    kind: z.literal("started"),
    mode: z.enum(["command", "daemon"]),
    executable: z.string(),
    arguments: z.array(z.string()),
    cwd: z.string(),
    environmentKeys: z.array(z.string()),
  })
  .strict();
const spawned = base
  .extend({ kind: z.literal("spawned"), pid: z.number().int().positive() })
  .strict();
const ended = base
  .extend({
    kind: z.literal("ended"),
    pid: z.number().int().positive().nullable(),
    exit: z.number().int().nullable(),
    signal: z.string().nullable(),
    error: z.string().nullable(),
  })
  .strict();
const baseline = z
  .object({ kind: z.literal("baseline"), at: z.iso.datetime(), baselineId: z.uuidv7() })
  .strict();
const entry = z.discriminatedUnion("kind", [started, spawned, ended, baseline]);
export type CommandLedgerEntry = z.infer<typeof entry>;

async function checkLedger(path: string): Promise<void> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o777) !== 0o600
  )
    throw new Error("owner-only mode0600 command ledger required");
  if (info.size > maximumLedgerBytes) throw new Error("bounded complete command ledger required");
}
export async function registerCommandRoot(root: string): Promise<() => void> {
  if (resolve(root) !== root || /(^|\/)\.(codex|codex-router|claude)(\/|$)/.test(root))
    throw new Error("isolated command root required");
  const directory = await lstat(root);
  if (
    !directory.isDirectory() ||
    directory.isSymbolicLink() ||
    directory.uid !== process.getuid?.() ||
    (directory.mode & 0o077) !== 0 ||
    (await realpath(root)) !== root
  )
    throw new Error("canonical owned private command root required");
  const path = `${root}/verification-commands.jsonl`;
  try {
    closeSync(openSync(path, "wx", 0o600));
  } catch (error: unknown) {
    if (!(error instanceof Error && "code" in error && error.code === "EEXIST")) throw error;
  }
  await checkLedger(path);
  await readCommandLedger(root);
  const registration = roots.get(root) ?? { path, users: 0 };
  registration.users += 1;
  roots.set(root, registration);
  let released = false;
  return (): void => {
    if (released) return;
    released = true;
    registration.users -= 1;
    if (registration.users === 0) roots.delete(root);
  };
}
export function beginCommandLedger(input: {
  executable: string;
  arguments: readonly string[];
  cwd: string;
  environment: NodeJS.ProcessEnv;
  mode: "command" | "daemon";
}):
  | {
      spawned(pid: number): void;
      ended(result: {
        pid: number | null;
        exit: number | null;
        signal: string | null;
        error: string | null;
      }): void;
    }
  | undefined {
  const matches = [...roots].filter(
    ([root]): boolean =>
      input.arguments.some(
        (argument): boolean => argument === root || argument.startsWith(`${root}/`),
      ) ||
      input.cwd === root ||
      input.cwd.startsWith(`${root}/`),
  );
  const paths = matches.map(([, registration]): string => registration.path);
  if (paths.length === 0) return undefined;
  const commandId = randomUUIDv7();
  const append = (value: CommandLedgerEntry): void => {
    for (const path of paths) appendFileSync(path, `${encode(value)}\n`, { mode: 0o600 });
  };
  append({
    kind: "started",
    at: new Date().toISOString(),
    commandId,
    mode: input.mode,
    executable: input.executable,
    arguments: [...input.arguments],
    cwd: input.cwd,
    environmentKeys: Object.keys(input.environment).sort(),
  });
  return {
    spawned(pid): void {
      append({ kind: "spawned", at: new Date().toISOString(), commandId, pid });
    },
    ended(result): void {
      append({ kind: "ended", at: new Date().toISOString(), commandId, ...result });
    },
  };
}
export async function readCommandLedger(root: string): Promise<CommandLedgerEntry[]> {
  const path = `${root}/verification-commands.jsonl`;
  await checkLedger(path);
  const text = await readFile(path, "utf8");
  if (Buffer.byteLength(text) > maximumLedgerBytes || (text !== "" && !text.endsWith("\n")))
    throw new Error("bounded complete command ledger required");
  return text
    .split("\n")
    .filter((line): boolean => line !== "")
    .map((line): CommandLedgerEntry => {
      // JSON protocol integers are exact, while process PIDs/exits are bounded JS integers.
      return entry.parse(decodeAuthoredJson(line));
    });
}
export async function markCommandBaseline(root: string): Promise<string> {
  const path = roots.get(root)?.path;
  if (path === undefined) throw new Error("baseline requires registered command root");
  const rows = await readCommandLedger(root);
  for (const row of rows)
    if (
      row.kind === "started" &&
      row.mode === "command" &&
      !rows.some(
        (candidate): boolean => candidate.kind === "ended" && candidate.commandId === row.commandId,
      )
    )
      throw new Error("cannot mark baseline over an unfinished command");
  const baselineId = randomUUIDv7();
  appendFileSync(
    path,
    `${encode({ kind: "baseline", baselineId, at: new Date().toISOString() })}\n`,
    { mode: 0o600 },
  );
  return baselineId;
}
export async function enrollInvocationsSince(root: string, baselineId: string): Promise<number> {
  const rows = await readCommandLedger(root);
  const indices = rows.flatMap((row, index): number[] =>
    row.kind === "baseline" && row.baselineId === baselineId ? [index] : [],
  );
  const index = indices[0];
  if (indices.length !== 1 || index === undefined)
    throw new Error("one recorded enrollment baseline required");
  const subsequent = rows.slice(index + 1);
  const starts = new Map<string, z.infer<typeof started>>();
  const finished = new Set<string>();
  for (const row of subsequent) {
    if (row.kind === "started") {
      if (starts.has(row.commandId)) throw new Error("duplicate command start");
      starts.set(row.commandId, row);
    } else if (row.kind === "ended") {
      if (finished.has(row.commandId)) throw new Error("duplicate command ending");
      finished.add(row.commandId);
    }
  }
  for (const start of starts.values())
    if (start.mode === "command" && !finished.has(start.commandId))
      throw new Error("bounded command outcome is missing");
  return [...starts.values()].filter((start): boolean =>
    start.arguments.some(
      (argument, offset): boolean =>
        argument === "fleet" && start.arguments[offset + 1] === "enroll",
    ),
  ).length;
}
