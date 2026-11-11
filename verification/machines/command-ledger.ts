/** Per-root command evidence for explicitly registered owned verification roots. */
import { appendFileSync, openSync, closeSync, type Stats } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import { randomUUIDv7 } from "node:crypto";
import { resolve } from "node:path";
import { z } from "zod";
import { decodeAuthoredJson, encode } from "../exact-json.ts";

const roots = new Map<string, { path: string; users: number }>();
// Bound an individual retained row, rather than the lifetime of an append-only ledger.
const maximumLedgerRowBytes = 512 * 1024 * 1024;
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

async function checkLedger(path: string): Promise<Stats> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o777) !== 0o600
  )
    throw new Error("owner-only mode0600 command ledger required");
  if (!Number.isSafeInteger(info.size) || info.size < 0)
    throw new Error("bounded complete command ledger required");
  return info;
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
  for await (const _row of readCommandLedgerEntries(root)) {
    // Validate every historical row without retaining every command argument.
  }
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
/** Validate one complete captured prefix; later appends belong to the next scan. */
export async function* readCommandLedgerEntries(
  root: string,
): AsyncGenerator<CommandLedgerEntry, void> {
  const path = `${root}/verification-commands.jsonl`;
  const expected = await checkLedger(path);
  const handle = await open(path, "r");
  try {
    const captured = await handle.stat();
    if (captured.dev !== expected.dev || captured.ino !== expected.ino)
      throw new Error("command ledger identity changed while opening");
    const byteLength = captured.size;
    if (!Number.isSafeInteger(byteLength) || byteLength < 0)
      throw new Error("bounded complete command ledger required");
    if (byteLength > 0) {
      const lastByte = Buffer.alloc(1);
      const last = await handle.read(lastByte, 0, 1, byteLength - 1);
      if (last.bytesRead !== 1 || lastByte[0] !== 10)
        throw new Error("bounded complete command ledger required");
    }
    const buffer = Buffer.alloc(64 * 1024);
    let position = 0;
    let fragments: Buffer[] = [];
    let rowBytes = 0;
    while (position < byteLength) {
      const { bytesRead } = await handle.read(
        buffer,
        0,
        Math.min(buffer.length, byteLength - position),
        position,
      );
      if (bytesRead === 0) throw new Error("complete command ledger prefix is missing");
      position += bytesRead;
      let consumed = 0;
      while (consumed < bytesRead) {
        const newline = buffer.indexOf(10, consumed);
        const terminal = newline >= consumed && newline < bytesRead;
        const end = terminal ? newline : bytesRead;
        const part = buffer.subarray(consumed, end);
        rowBytes += part.length;
        if (rowBytes > maximumLedgerRowBytes)
          throw new Error("bounded complete command ledger row required");
        if (!terminal) {
          fragments.push(Buffer.from(part));
          break;
        }
        const row = fragments.length === 0 ? part : Buffer.concat([...fragments, part], rowBytes);
        if (rowBytes > 0)
          yield entry.parse(structuredClone(decodeAuthoredJson(row.toString("utf8"))));
        fragments = [];
        rowBytes = 0;
        consumed = newline + 1;
      }
    }
    if (rowBytes !== 0) throw new Error("bounded complete command ledger required");
    const current = await checkLedger(path);
    if (current.dev !== captured.dev || current.ino !== captured.ino || current.size < byteLength)
      throw new Error("complete command ledger prefix changed during read");
  } finally {
    await handle.close();
  }
}
export async function readCommandLedger(root: string): Promise<CommandLedgerEntry[]> {
  const rows: CommandLedgerEntry[] = [];
  for await (const row of readCommandLedgerEntries(root)) rows.push(row);
  return rows;
}
export async function markCommandBaseline(root: string): Promise<string> {
  const path = roots.get(root)?.path;
  if (path === undefined) throw new Error("baseline requires registered command root");
  const starts = new Set<string>();
  const finished = new Set<string>();
  for await (const row of readCommandLedgerEntries(root)) {
    if (row.kind === "started" && row.mode === "command") starts.add(row.commandId);
    else if (row.kind === "ended") finished.add(row.commandId);
  }
  for (const commandId of starts)
    if (!finished.has(commandId))
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
  let baselineMatches = 0;
  let enrollments = 0;
  const starts = new Map<string, "command" | "daemon">();
  const finished = new Set<string>();
  for await (const row of readCommandLedgerEntries(root)) {
    if (row.kind === "baseline" && row.baselineId === baselineId) {
      baselineMatches += 1;
      if (baselineMatches > 1) throw new Error("one recorded enrollment baseline required");
    } else if (baselineMatches === 0) {
      continue;
    } else if (row.kind === "started") {
      if (starts.has(row.commandId)) throw new Error("duplicate command start");
      starts.set(row.commandId, row.mode);
      if (
        row.arguments.some(
          (argument, offset): boolean =>
            argument === "fleet" && row.arguments[offset + 1] === "enroll",
        )
      )
        enrollments += 1;
    } else if (row.kind === "ended") {
      if (finished.has(row.commandId)) throw new Error("duplicate command ending");
      finished.add(row.commandId);
    }
  }
  if (baselineMatches !== 1) throw new Error("one recorded enrollment baseline required");
  for (const [commandId, mode] of starts)
    if (mode === "command" && !finished.has(commandId))
      throw new Error("bounded command outcome is missing");
  return enrollments;
}
