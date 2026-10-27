// One child under a real OS pseudoterminal: spawn, raw input and output with their
// positions, resize, a bounded output capture, exit, and cleanup of only that child.
// Output is the raw text the PTY delivered (decoded as UTF-8), not a rendered screen;
// a match in it is not a claim about what a terminal would display.
// On macOS, node-pty 1.1.0's prebuilt spawn-helper installs without its execute bit
// (every spawn fails with "posix_spawnp failed"); run `pnpm --dir verification run build:pty` after
// installing to build it from source.
import { spawn, type IDisposable, type IPty } from "node-pty";
import { z } from "zod";

const terminalDimension = z.number().int().min(1).max(10_000);
// setTimeout's own upper bound; a larger value would fire immediately.
const deadlineMilliseconds = z.number().int().min(1).max(2_147_483_647);

export const ptyLaunchSchema = z
  .object({
    executable: z.string().min(1),
    arguments: z.array(z.string()).readonly(),
    cwd: z.string().min(1),
    environment: z.record(z.string(), z.string()).readonly(),
    columns: terminalDimension,
    rows: terminalDimension,
    outputLimitBytes: z
      .number()
      .int()
      .min(1)
      .max(256 * 1024 * 1024),
  })
  .readonly();
export type PtyLaunch = z.infer<typeof ptyLaunchSchema>;

export interface PtyExit {
  readonly exitCode: number;
  readonly signal: number | undefined;
}

/** Input as written, with the output length at the moment it was written. */
export interface PtyInput {
  readonly outputOffset: number;
  readonly data: string;
}

export type PtyOutputWait =
  | { readonly kind: "matched"; readonly offset: number }
  | { readonly kind: "exited"; readonly exit: PtyExit }
  | { readonly kind: "overflowed" }
  | { readonly kind: "deadline" };

export type PtyExitWait =
  | { readonly kind: "exited"; readonly exit: PtyExit }
  | { readonly kind: "deadline" };

export class PtyProcess {
  readonly pid: number;
  readonly exited: Promise<PtyExit>;
  private readonly terminal: IPty;
  private readonly subscriptions: readonly IDisposable[];
  private readonly changeListeners = new Set<() => void>();
  private readonly writtenInputs: PtyInput[] = [];
  private capturedOutput = "";
  private capturedBytes = 0;
  private outputOverflowed = false;
  private observedExit: PtyExit | undefined;

  private constructor(launch: PtyLaunch) {
    this.terminal = spawn(launch.executable, [...launch.arguments], {
      cols: launch.columns,
      rows: launch.rows,
      cwd: launch.cwd,
      env: { ...launch.environment },
    });
    this.pid = this.terminal.pid;
    let resolveExit: (exit: PtyExit) => void = (): void => {};
    this.exited = new Promise<PtyExit>((resolve): void => {
      resolveExit = resolve;
    });
    this.subscriptions = [
      this.terminal.onData((chunk: string): void => {
        this.receiveOutput(chunk, launch.outputLimitBytes);
      }),
      this.terminal.onExit((event: { exitCode: number; signal?: number }): void => {
        this.observedExit = { exitCode: event.exitCode, signal: event.signal };
        for (const subscription of this.subscriptions) subscription.dispose();
        resolveExit(this.observedExit);
        this.notifyChange();
      }),
    ];
  }

  /** Validates the launch before anything is spawned. */
  static start(launch: PtyLaunch): PtyProcess {
    return new PtyProcess(ptyLaunchSchema.parse(launch));
  }

  /** The raw output so far; its length is the position later waits start from. */
  get output(): string {
    return this.capturedOutput;
  }

  get inputs(): readonly PtyInput[] {
    return this.writtenInputs;
  }

  get overflowed(): boolean {
    return this.outputOverflowed;
  }

  write(data: string): void {
    this.refuseAfterExit("input");
    this.writtenInputs.push({ outputOffset: this.capturedOutput.length, data });
    this.terminal.write(data);
  }

  resize(columns: number, rows: number): void {
    const size = z.object({ columns: terminalDimension, rows: terminalDimension }).parse({
      columns,
      rows,
    });
    this.refuseAfterExit("resize");
    this.terminal.resize(size.columns, size.rows);
  }

  /**
   * Waits for a non-empty `marker` at or after `afterOffset`, a position taken from
   * `output.length`; earlier (stale) output never matches. Once the capture overflowed,
   * every observation fails, even for a marker that arrived before the overflow.
   */
  waitForOutput(marker: string, afterOffset: number, deadline: number): Promise<PtyOutputWait> {
    const request = z
      .object({
        marker: z.string().min(1),
        afterOffset: z.number().int().min(0).max(this.capturedOutput.length),
        deadline: deadlineMilliseconds,
      })
      .parse({ marker, afterOffset, deadline });
    return this.waitUntil((): PtyOutputWait | undefined => {
      if (this.outputOverflowed) return { kind: "overflowed" };
      const offset = this.capturedOutput.indexOf(request.marker, request.afterOffset);
      if (offset >= 0) return { kind: "matched", offset };
      if (this.observedExit !== undefined) return { kind: "exited", exit: this.observedExit };
      return undefined;
    }, request.deadline);
  }

  waitForExit(deadline: number): Promise<PtyExitWait> {
    return this.waitUntil(
      (): PtyExitWait | undefined =>
        this.observedExit === undefined ? undefined : { kind: "exited", exit: this.observedExit },
      deadlineMilliseconds.parse(deadline),
    );
  }

  /** Ends only this child: SIGTERM, then SIGKILL after the deadline, then confirms it is gone. */
  async stop(deadline: number): Promise<PtyExit> {
    if (this.observedExit === undefined) this.terminal.kill("SIGTERM");
    let ending = await this.waitForExit(deadline);
    if (ending.kind === "deadline") {
      this.terminal.kill("SIGKILL");
      ending = await this.waitForExit(deadline);
    }
    if (ending.kind === "deadline") throw new Error("owned PTY child did not exit after SIGKILL");
    if (processExists(this.pid)) throw new Error("owned PTY child PID still exists after exit");
    return ending.exit;
  }

  private receiveOutput(chunk: string, limitBytes: number): void {
    if (this.outputOverflowed) return;
    const chunkBytes = Buffer.byteLength(chunk);
    if (this.capturedBytes + chunkBytes > limitBytes) {
      // The capture is incomplete from here on: fail the observation and end the child.
      this.outputOverflowed = true;
      this.terminal.kill("SIGKILL");
    } else {
      this.capturedOutput += chunk;
      this.capturedBytes += chunkBytes;
    }
    this.notifyChange();
  }

  private refuseAfterExit(operation: string): void {
    if (this.observedExit !== undefined)
      throw new Error(`PTY child has exited; ${operation} refused`);
  }

  private notifyChange(): void {
    // A settled wait deletes its own listener; deleting the visited entry is safe in a Set.
    for (const listener of this.changeListeners) listener();
  }

  private waitUntil<TResult>(
    settled: () => TResult | undefined,
    deadline: number,
  ): Promise<TResult | { readonly kind: "deadline" }> {
    return new Promise((resolve): void => {
      const finish = (result: TResult | { readonly kind: "deadline" }): void => {
        clearTimeout(timer);
        this.changeListeners.delete(check);
        resolve(result);
      };
      const check = (): void => {
        const result = settled();
        if (result !== undefined) finish(result);
      };
      // The deadline only bounds a failure; success is decided by output and exit events.
      const timer = setTimeout((): void => {
        finish({ kind: "deadline" });
      }, deadline);
      this.changeListeners.add(check);
      check();
    });
  }
}

function processExists(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (failure: unknown) {
    if (failure instanceof Error && "code" in failure && failure.code === "ESRCH") return false;
    throw failure;
  }
}
