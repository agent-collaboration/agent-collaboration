/**
 * Recorded-frame fixtures for the moved O5 journeys (`/resume`, the tier pick, approval): small
 * wire builders with switches that seed one defect at a time. Used by tests only, never by a run.
 */
import type { WireFrame } from "./turn-skeleton.ts";

export const LAUNCH_THREAD = "019a0000-0000-7000-8000-0000000000a1";
export const OTHER_THREAD = "019a0000-0000-7000-8000-0000000000b2";

class FrameList {
  private readonly frames: WireFrame[] = [];
  private readonly connection: number;
  constructor(connection: number) {
    this.connection = connection;
  }
  push(direction: WireFrame["direction"], message: unknown): this {
    this.frames.push({ seq: this.frames.length, connection: this.connection, direction, message });
    return this;
  }
  build(): readonly WireFrame[] {
    return this.frames;
  }
}

export interface ResumeRunOptions {
  /** The thread the `/resume` pick resumes; defaults to the declared other thread. */
  readonly pickedThread?: string;
  readonly skipPick?: boolean;
  readonly refusePick?: boolean;
  readonly startNewThreadInstead?: boolean;
  readonly connection?: number;
  /**
   * How the CLI launched: a Router target resumes its Session; the real-Codex reference starts its
   * own thread (assignment Correction 29). Defaults to the resume.
   */
  readonly launch?: "resume" | "start";
  /** How the server answers the pick: with the picked thread (default), not at all, empty, or another thread. */
  readonly pickAnswer?: "answered" | "none" | "empty" | "other";
}

/** The launch (a resume, or the reference's own start), then the `/resume` pick. */
export function resumeRun(options: ResumeRunOptions = {}): readonly WireFrame[] {
  const wire = new FrameList(options.connection ?? 0)
    .push(
      "cliToServer",
      options.launch === "start"
        ? { id: 1, method: "thread/start", params: { model: "gpt-6-luna" } }
        : { id: 1, method: "thread/resume", params: { threadId: LAUNCH_THREAD } },
    )
    .push("serverToCli", { id: 1, result: { thread: { id: LAUNCH_THREAD } } });
  if (options.skipPick === true) return wire.build();
  if (options.startNewThreadInstead === true)
    return wire
      .push("cliToServer", { id: 2, method: "thread/start", params: {} })
      .push("serverToCli", { id: 2, result: { thread: { id: "fresh" } } })
      .build();
  const picked = options.pickedThread ?? OTHER_THREAD;
  wire.push("cliToServer", { id: 2, method: "thread/resume", params: { threadId: picked } });
  return options.refusePick === true
    ? wire
        .push("serverToCli", { id: 2, error: { code: -32600, message: "no such thread" } })
        .build()
    : options.pickAnswer === "none"
      ? wire.build()
      : wire
          .push("serverToCli", {
            id: 2,
            result:
              options.pickAnswer === "empty"
                ? {}
                : { thread: { id: options.pickAnswer === "other" ? "someone-else" : picked } },
          })
          .build();
}

export interface TierRunOptions {
  /** The tier the last settings frame carries; null is a cleared tier. */
  readonly tier: string | null;
  readonly omitSettings?: boolean;
  readonly connection?: number;
}

/** The `/model` apply and the tier pick as the CLI receives them: settings updates. */
export function tierRun(options: TierRunOptions): readonly WireFrame[] {
  const wire = new FrameList(options.connection ?? 0);
  if (options.omitSettings === true)
    return wire.push("cliToServer", { id: 1, method: "x" }).build();
  return wire
    .push("cliToServer", { id: 7, method: "thread/settings/update", params: { threadId: "t" } })
    .push("serverToCli", {
      id: 7,
      result: { threadSettings: { model: "gpt-x", effort: "low", serviceTier: options.tier } },
    })
    .build();
}

export interface ApprovalRunOptions {
  readonly prompts?: number;
  /** The CLI's answer; undefined means it never answered (a timeout). */
  readonly decision?: string;
  readonly connection?: number;
}

/** One `requestApproval` server request and the CLI's answer. */
export function approvalRun(options: ApprovalRunOptions = {}): readonly WireFrame[] {
  const wire = new FrameList(options.connection ?? 0);
  for (let index = 0; index < (options.prompts ?? 1); index += 1) {
    wire.push("serverToCli", {
      id: `req-${index}`,
      method: "item/commandExecution/requestApproval",
      params: { turnId: "T1" },
    });
    if (options.decision !== undefined)
      wire.push("cliToServer", { id: `req-${index}`, result: { decision: options.decision } });
  }
  return wire.build();
}
