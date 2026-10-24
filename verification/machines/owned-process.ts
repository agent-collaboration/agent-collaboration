import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { createWriteStream, openSync, closeSync } from "node:fs";
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { createInterface } from "node:readline";
import type { Artifact } from "../declaration.ts";
import { Receipt } from "../receipt.ts";

export function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}
export async function fingerprint(path: string): Promise<string> {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
}
export async function privateDirectory(path: string): Promise<void> {
  const info = await lstat(path);
  if (
    !info.isDirectory() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    (await realpath(path)) !== path
  )
    throw new Error("canonical owner-private directory required");
}
export async function qualifyArtifact(artifact: Artifact): Promise<void> {
  if ((await fingerprint(artifact.path)) !== artifact.sha256)
    throw new Error("artifact hash mismatch");
  execFileSync("/usr/bin/codesign", ["--verify", "--strict", artifact.path], { stdio: "pipe" });
  const metadata = await command("/usr/bin/codesign", ["-d", "--verbose=4", artifact.path]);
  if (
    !metadata.stderr.includes(`Identifier=${artifact.identifier}\n`) ||
    !metadata.stderr.includes(`TeamIdentifier=${artifact.team}\n`)
  )
    throw new Error("artifact signature identity mismatch");
}
export async function command(
  executable: string,
  arguments_: readonly string[],
  options: {
    timeoutSeconds?: number;
    cwd?: string;
    input?: string;
    environment?: NodeJS.ProcessEnv;
  } = {},
): Promise<{ stdout: string; stderr: string; exit: number | null }> {
  const child = spawn(executable, arguments_, {
    stdio: ["pipe", "pipe", "pipe"],
    signal: AbortSignal.timeout((options.timeoutSeconds ?? 30) * 1000),
    ...(options.environment === undefined ? {} : { env: options.environment }),
    ...(options.cwd === undefined ? {} : { cwd: options.cwd }),
  });
  let stdout = "";
  let stderr = "";
  child.stdin.on("error", (): void => {});
  child.stdin.end(options.input);
  child.stdout.on("data", (chunk: Buffer): void => {
    stdout += chunk.toString();
    if (stdout.length > 16 * 1024 * 1024) child.kill("SIGTERM");
  });
  child.stderr.on("data", (chunk: Buffer): void => {
    stderr += chunk.toString();
    if (stderr.length > 1024 * 1024) child.kill("SIGTERM");
  });
  const exit = await new Promise<number | null>((resolve, reject): void => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  return { stdout, stderr, exit };
}
export class OwnedProcess {
  readonly child: ChildProcess;
  readonly identity: string;
  readonly done: Promise<number | null>;
  readonly receipt: Receipt;
  private readonly descendants = new Map<number, string>();
  private readonly lines: string[] = [];
  private outputOverflow = false;
  private readonly listeners = new Set<(line: string) => void>();
  constructor(input: {
    executable: string;
    arguments: readonly string[];
    environment: NodeJS.ProcessEnv;
    cwd: string;
    name: string;
    receipt: Receipt;
  }) {
    execFileSync("/bin/ps", ["-p", String(process.pid), "-o", "pid=,lstart=,comm="], {
      stdio: "pipe",
    });
    this.receipt = input.receipt;
    // Fail before spawning if either exclusive evidence file cannot be created.
    const outputDescriptor = openSync(
      `${input.receipt.directory}/${input.name}.stdout`,
      "wx",
      0o600,
    );
    let errorDescriptor: number;
    try {
      errorDescriptor = openSync(`${input.receipt.directory}/${input.name}.stderr`, "wx", 0o600);
    } catch (failure: unknown) {
      closeSync(outputDescriptor);
      throw failure;
    }
    const output = createWriteStream("", { fd: outputDescriptor });
    const error = createWriteStream("", { fd: errorDescriptor });
    for (const stream of [output, error])
      stream.on("error", (): void => {
        this.outputOverflow = true;
      });
    this.child = spawn(input.executable, input.arguments, {
      cwd: input.cwd,
      env: input.environment,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child.stdout?.pipe(output);
    this.child.stderr?.pipe(error);
    if (this.child.stdout !== null)
      createInterface({ input: this.child.stdout }).on("line", (line: string): void => {
        if (this.listeners.size > 0) for (const listener of this.listeners) listener(line);
        else if (this.lines.length < 4096) this.lines.push(line);
        else this.outputOverflow = true;
      });
    if (this.child.pid === undefined) throw new Error("child PID unavailable");
    this.identity = execFileSync(
      "/bin/ps",
      ["-p", String(this.child.pid), "-o", "pid=,lstart=,comm="],
      { encoding: "utf8" },
    );
    this.done = new Promise<number | null>((resolve, reject): void => {
      this.child.once("error", reject);
      this.child.once("exit", resolve);
    });
  }
  get pid(): number {
    if (this.child.pid === undefined) throw new Error("child PID missing");
    return this.child.pid;
  }
  checkOutput(): void {
    if (this.outputOverflow)
      throw new Error("owned output capture incomplete; observation invalid");
  }
  async line(seconds = 30): Promise<string> {
    const deadline = Date.now() + seconds * 1000;
    while (Date.now() < deadline) {
      this.checkOutput();
      const line = this.lines.shift();
      if (line !== undefined) return line;
      if (this.child.exitCode !== null || this.child.signalCode !== null)
        throw new Error("owned child ended before reply");
      await setTimeout(25);
    }
    throw new Error("owned reply deadline exceeded");
  }
  subscribe(listener: (line: string) => void): () => void {
    this.checkOutput();
    this.listeners.add(listener);
    for (const line of this.lines.splice(0)) listener(line);
    return (): void => {
      this.listeners.delete(listener);
    };
  }
  capture(): void {
    const lines = execFileSync("/bin/ps", ["-axo", "pid=,ppid=,lstart=,comm="], {
      encoding: "utf8",
    })
      .trim()
      .split("\n");
    const selected = new Set([this.pid]);
    for (let pass = 0; pass < 20; pass += 1)
      for (const line of lines) {
        const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.+)$/);
        if (
          match?.[1] !== undefined &&
          match[2] !== undefined &&
          match[3] !== undefined &&
          selected.has(Number(match[2]))
        ) {
          selected.add(Number(match[1]));
          this.descendants.set(Number(match[1]), match[3]);
        }
      }
  }
  async stop(): Promise<void> {
    this.capture();
    if (this.child.exitCode === null && this.child.signalCode === null) {
      const observed = execFileSync(
        "/bin/ps",
        ["-p", String(this.pid), "-o", "pid=,lstart=,comm="],
        { encoding: "utf8" },
      );
      if (observed !== this.identity)
        throw new Error("owned PID identity changed; refusing signal");
      this.child.kill("SIGTERM");
    }
    const result = await Promise.race([this.done, setTimeout(20_000, "timeout", { ref: false })]);
    if (result === "timeout") throw new Error("owned process cleanup timed out");
    const descendantDeadline = Date.now() + 20_000;
    for (const [pid, identity] of this.descendants) {
      let remaining = await command("/bin/ps", ["-p", String(pid), "-o", "lstart=,comm="]);
      while (
        remaining.exit === 0 &&
        remaining.stdout.trim() === identity.trim() &&
        Date.now() < descendantDeadline
      ) {
        await setTimeout(100);
        remaining = await command("/bin/ps", ["-p", String(pid), "-o", "lstart=,comm="]);
      }
      if (remaining.exit === 0) {
        await this.receipt.record("remainingCapturedPid", {
          pid,
          identityMatches: remaining.stdout.trim() === identity.trim(),
        });
        throw new Error("captured descendant PID remains or changed identity; cleanup unverified");
      }
    }
    await this.receipt.record("processStopped", {
      pid: this.pid,
      exit: result,
      descendants: this.descendants.size,
    });
    this.checkOutput();
  }
}
