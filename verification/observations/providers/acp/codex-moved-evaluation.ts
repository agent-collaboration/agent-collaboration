/**
 * Wire checks for the O5 journeys that moved to D3: `/resume`, the service-tier pick and two
 * windows. Each reads the wire tee's recording (one CLI connection, or both windows), never the
 * grid. Approvals are judged by codex-approval-compare.ts. `loadMovedRun` reads a run directory's `moved-run.json` (what the run declared and
 * expected) plus its recording; `evaluateMovedRun` returns every violation.
 *
 * Built and unit-tested only. `thread/resume` as the method a `/resume` pick sends is an
 * assumption read from the app-server protocol, unobserved on the real 0.166.0 CLI until a
 * discovery run.
 */
import { readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { z } from "zod";

import { parseWireRecording } from "./codex-wire-tee.ts";
import { checkTwoWindows } from "./codex-window-evaluation.ts";
import { reduceTurnSkeleton } from "./turn-skeleton.ts";
import type { TurnSkeleton, WireFrame } from "./turn-skeleton.ts";

export const MOVED_RUN_FILE = "moved-run.json";
/** The request a `/resume` pick sends for the chosen thread. */
export const RESUME_METHOD = "thread/resume";
/** Requests that would open a new thread instead of resuming one. */
const NEW_THREAD_METHODS: ReadonlySet<string> = new Set(["thread/start", "thread/fork"]);

export interface MovedViolation {
  readonly journey: "resume" | "tier" | "windows";
  readonly detail: string;
}

const requestSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  method: z.string(),
  params: z.unknown().optional(),
});
const threadParamsSchema = z.looseObject({ threadId: z.string() });
const responseSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  result: z.unknown().optional(),
  error: z.unknown().optional(),
});
const threadResultSchema = z.looseObject({ thread: z.looseObject({ id: z.string() }) });

interface ResumeRequest {
  readonly seq: number;
  readonly connection: number;
  readonly id: string;
  readonly threadId: string | undefined;
}

function cliRequests(frames: readonly WireFrame[], method: (name: string) => boolean) {
  return [...frames]
    .sort((left, right) => left.seq - right.seq)
    .flatMap((frame) => {
      if (frame.direction !== "cliToServer") return [];
      const request = requestSchema.safeParse(frame.message);
      return request.success && method(request.data.method)
        ? [{ frame, request: request.data }]
        : [];
    });
}

function resumeRequests(frames: readonly WireFrame[]): ResumeRequest[] {
  return cliRequests(frames, (name) => name === RESUME_METHOD).map(({ frame, request }) => ({
    seq: frame.seq,
    connection: frame.connection,
    id: String(request.id),
    threadId: threadParamsSchema.safeParse(request.params).data?.threadId,
  }));
}

/** The server's answer to one CLI request on its own connection: none, an error, or a result. */
type RequestAnswer =
  | { readonly kind: "none" }
  | { readonly kind: "error" }
  | { readonly kind: "result"; readonly threadId: string | undefined };

function answerTo(frames: readonly WireFrame[], connection: number, id: string): RequestAnswer {
  for (const frame of frames) {
    if (frame.direction !== "serverToCli" || frame.connection !== connection) continue;
    const response = responseSchema.safeParse(frame.message);
    if (!response.success || String(response.data.id) !== id) continue;
    if (response.data.error !== undefined && response.data.error !== null) return { kind: "error" };
    if (response.data.result === undefined) continue;
    return {
      kind: "result",
      threadId: threadResultSchema.safeParse(response.data.result).data?.thread.id,
    };
  }
  return { kind: "none" };
}

export interface ResumeExpectation {
  /**
   * The thread the CLI launched on: a Router target's `codex resume <id>`, or the thread the
   * real-Codex reference's own first `thread/start` opened (assignment Correction 29).
   */
  readonly launchThreadId: string;
  /** The thread the `/resume` pick must open: another one. */
  readonly expectedThreadId: string;
}

/**
 * `/resume`: after the launch, the CLI resumed exactly the declared other thread, the server answered
 * that request with that thread (review F7), and no thread was started or forked instead. The launch
 * itself is a resume of the launch thread, or, for the real-Codex reference, its first thread-opening
 * request: a `thread/start` answered with the launch thread (review F3).
 */
export function checkResumedThread(
  frames: readonly WireFrame[],
  expectation: ResumeExpectation,
): MovedViolation[] {
  const violations: MovedViolation[] = [];
  const resume = (detail: string): void => {
    violations.push({ journey: "resume", detail });
  };
  if (expectation.launchThreadId === expectation.expectedThreadId)
    resume("the thread to resume is the launch thread; the pick could not be told apart");
  const requests = resumeRequests(frames);
  const picked = requests.filter((request) => request.threadId !== expectation.launchThreadId);
  const wrong = picked.filter((request) => request.threadId !== expectation.expectedThreadId);
  for (const request of wrong)
    resume(
      `resumed thread ${request.threadId} at ${request.seq}, not ${expectation.expectedThreadId}`,
    );
  const right = picked.filter((request) => request.threadId === expectation.expectedThreadId);
  if (right.length === 0) resume(`no ${RESUME_METHOD} for ${expectation.expectedThreadId}`);
  if (right.length > 1) resume(`${right.length} resumes of ${expectation.expectedThreadId}`);
  for (const request of right) {
    const answer = answerTo(frames, request.connection, request.id);
    if (answer.kind === "error") resume(`resume of ${request.threadId} was refused`);
    else if (answer.kind === "none")
      resume(`resume of ${request.threadId} at ${request.seq} was never answered`);
    else if (answer.threadId !== expectation.expectedThreadId)
      resume(
        `resume of ${request.threadId} at ${request.seq} answered with thread ${String(answer.threadId)}, not ${expectation.expectedThreadId}`,
      );
  }
  const opened = cliRequests(
    frames,
    (name) => NEW_THREAD_METHODS.has(name) || name === RESUME_METHOD,
  );
  const launch = opened[0];
  for (const { frame, request } of cliRequests(frames, (name) => NEW_THREAD_METHODS.has(name))) {
    const isLaunchStart =
      launch !== undefined &&
      frame.seq === launch.frame.seq &&
      request.method === "thread/start" &&
      (() => {
        const answer = answerTo(frames, frame.connection, String(request.id));
        return answer.kind === "result" && answer.threadId === expectation.launchThreadId;
      })();
    if (!isLaunchStart) resume(`${request.method} at ${frame.seq}: a new thread, not a resume`);
  }
  return violations;
}

/**
 * The service-tier pick: the last settings the CLI received carry exactly the picked tier. A
 * recording with no settings frame, or one still showing another tier, fails. An omitted
 * `serviceTier` keeps the previous tier and an explicit null clears it (turn-skeleton.ts settingsFrom).
 */
export function checkTierOnWire(skeleton: TurnSkeleton, wireTier: string): MovedViolation[] {
  const observed = skeleton.lastSettings;
  if (observed === undefined)
    return [{ journey: "tier", detail: "no thread settings reached the CLI after the pick" }];
  return observed.serviceTier === wireTier
    ? []
    : [
        {
          journey: "tier",
          detail: `service tier on the wire is ${JSON.stringify(observed.serviceTier)}, picked ${JSON.stringify(wireTier)}`,
        },
      ];
}

const movedRunBaseFields = {
  // The tee recording, absolute or relative to the run directory.
  wire_recording: z.string().min(1),
};

/** What a moved-journey run declared, written next to its recording by the run. */
export const movedRunRecordSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("resume"),
    ...movedRunBaseFields,
    launch_thread_id: z.string().min(1),
    expected_thread_id: z.string().min(1),
  }),
  z.strictObject({
    kind: z.literal("tier"),
    ...movedRunBaseFields,
    wire_value: z.string().min(1),
  }),
  z.strictObject({
    kind: z.literal("windows"),
    ...movedRunBaseFields,
    // The tee connections of the driving window and of the watching window.
    window_a: z.number().int().nonnegative(),
    window_b: z.number().int().nonnegative(),
  }),
]);
export type MovedRunRecord = z.infer<typeof movedRunRecordSchema>;

export interface LoadedMovedRun {
  readonly record: MovedRunRecord;
  readonly frames: readonly WireFrame[];
}

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8")) as unknown;
}

/** Reads one run directory; a malformed record, an empty or multi-connection recording rejects it. */
export function loadMovedRun(directory: string): LoadedMovedRun {
  const record = movedRunRecordSchema.parse(readJson(join(directory, MOVED_RUN_FILE)));
  const path = isAbsolute(record.wire_recording)
    ? record.wire_recording
    : join(directory, record.wire_recording);
  const frames = parseWireRecording(readFileSync(path, "utf8"));
  if (frames.length === 0) throw new Error("the wire recording holds no frames");
  const connections = new Set(frames.map((frame) => frame.connection)).size;
  if (record.kind !== "windows" && connections !== 1)
    throw new Error("a one-window moved run needs exactly one CLI connection");
  if (record.kind === "windows" && connections < 2)
    throw new Error("a two-window run needs a recording from both windows");
  return { record, frames };
}

/** Every violation of one moved-journey run; empty means the run passes its wire checks. */
export function evaluateMovedRun(run: LoadedMovedRun): MovedViolation[] {
  const { record } = run;
  if (record.kind === "resume")
    return checkResumedThread(run.frames, {
      launchThreadId: record.launch_thread_id,
      expectedThreadId: record.expected_thread_id,
    });
  if (record.kind === "windows")
    return checkTwoWindows(run.frames, { windowA: record.window_a, windowB: record.window_b }).map(
      (violation) => ({ journey: "windows", detail: `${violation.rule}: ${violation.detail}` }),
    );
  return checkTierOnWire(reduceTurnSkeleton(run.frames), record.wire_value);
}
