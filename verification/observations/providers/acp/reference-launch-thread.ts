/**
 * The real-Codex reference's launch thread (assignment Correction 33). After a turn, real Codex
 * 0.166.0 starts an ephemeral thread of its own, `thread/start` with `threadSource: "thread_title"`,
 * and runs one title-generation turn in it (D0 r1 c32 wire seq 137-153). That is Codex's
 * behavior, not the script's, so the reference's rules (k1-k5, compare, usage) judge only the
 * launch thread: the one the CLI's own first `thread/start` opened. A thread is set aside **only**
 * when it declares `threadSource: "thread_title"`. Any other extra thread is a violation.
 *
 * Two more rules from D0 r1 c46 (assignment Correction 48):
 * - The CLI reads the user's other threads for its history view (`thread/read`,
 *   `thread/turns/list`). Those read-only requests, and their replies, are set aside, not judged.
 *   Derived evidence keeps only their method, sequence and a thread-id hash: the replies carry the
 *   owner's other sessions and are never read or quoted. Any other request or notification naming
 *   another thread is still a violation.
 * - A title thread's turn is the turn/start that names it, never "the next turn after its start":
 *   real Codex interleaves the launch's own turn between them. Every turn/start into the launch
 *   thread carries the CLI's client message id; one without it is a Codex-internal turn in the
 *   launch thread, a violation.
 */
import { createHash } from "node:crypto";

import { z } from "zod";

import type { WireFrame } from "./turn-skeleton.ts";

/** The `threadSource` Codex declares on its own title-generation thread. */
export const TITLE_THREAD_SOURCE = "thread_title";

/** The read-only history requests set aside when they name another thread (Correction 48). */
export const SET_ASIDE_READ_METHODS: ReadonlySet<string> = new Set([
  "thread/read",
  "thread/turns/list",
]);

/** One set-aside read, as derived evidence: never its body or its thread id in clear. */
export interface SetAsideRead {
  readonly seq: number;
  readonly method: string;
  /** The first 16 hex digits of the read thread id's SHA-256. */
  readonly threadHash: string;
}

export interface LaunchThreadSelection {
  /** The thread the CLI's own first thread/start opened; undefined if none was answered. */
  readonly launchThreadId: string | undefined;
  /** The recording without the title threads' frames and the set-aside reads. */
  readonly frames: readonly WireFrame[];
  readonly violations: readonly string[];
  readonly setAsideReads: readonly SetAsideRead[];
}

const requestSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  method: z.string(),
  params: z.unknown().optional(),
});
const replySchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  result: z.unknown().optional(),
});
const startedThreadSchema = z.looseObject({ thread: z.looseObject({ id: z.string() }) });
const threadIdSchema = z.looseObject({ threadId: z.string() });
const sourceSchema = z.looseObject({ threadSource: z.unknown().optional() });
const notificationSchema = z.looseObject({ method: z.string(), params: z.unknown().optional() });

interface ThreadStart {
  readonly seq: number;
  readonly connection: number;
  readonly id: string | number;
  readonly source: unknown;
}

/**
 * A request's identity on its connection. JSON-RPC ids keep their JSON type: numeric `7` and string
 * `"7"` are different requests (review F16), so the key never stringifies the id itself.
 */
function key(connection: number, id: string | number | undefined): string {
  return JSON.stringify([connection, id ?? null]);
}

/** The thread a request's params or a notification's params name, if any. */
function namedThread(frame: WireFrame): string | undefined {
  if (frame.direction === "cliToServer") {
    const request = requestSchema.safeParse(frame.message);
    return request.success
      ? threadIdSchema.safeParse(request.data.params).data?.threadId
      : undefined;
  }
  const notification = notificationSchema.safeParse(frame.message);
  if (!notification.success) return undefined;
  return (
    threadIdSchema.safeParse(notification.data.params).data?.threadId ??
    startedThreadSchema.safeParse(notification.data.params).data?.thread.id
  );
}

function threadStarts(ordered: readonly WireFrame[]): ThreadStart[] {
  return ordered.flatMap((frame) => {
    if (frame.direction !== "cliToServer") return [];
    const request = requestSchema.safeParse(frame.message);
    if (!request.success || request.data.method !== "thread/start") return [];
    return [
      {
        seq: frame.seq,
        connection: frame.connection,
        id: request.data.id,
        source: sourceSchema.safeParse(request.data.params).data?.threadSource,
      },
    ];
  });
}

/** The thread a start's successful reply names, on the start's own connection. */
function replyThreadOf(ordered: readonly WireFrame[], start: ThreadStart): string | undefined {
  for (const frame of ordered) {
    if (frame.direction !== "serverToCli" || frame.connection !== start.connection) continue;
    const reply = replySchema.safeParse(frame.message);
    if (
      reply.success &&
      key(frame.connection, reply.data.id) === key(start.connection, start.id) &&
      reply.data.result !== undefined
    )
      return startedThreadSchema.safeParse(reply.data.result).data?.thread.id;
  }
  return undefined;
}

/** The launch thread, Codex's declared title threads, and every start that is neither. */
function classifyStarts(ordered: readonly WireFrame[]): {
  readonly launchThreadId: string | undefined;
  readonly titleThreads: ReadonlyMap<string, ThreadStart>;
  readonly violations: readonly string[];
} {
  const violations: string[] = [];
  const [launch, ...others] = threadStarts(ordered);
  if (launch !== undefined && launch.source === TITLE_THREAD_SOURCE)
    violations.push(
      `the first thread/start at ${launch.seq} is Codex's title thread, not the launch`,
    );
  const launchThreadId = launch === undefined ? undefined : replyThreadOf(ordered, launch);
  if (launchThreadId === undefined)
    violations.push("the reference recorded no answered thread/start for its launch thread");
  const titleThreads = new Map<string, ThreadStart>();
  for (const start of others) {
    const thread = replyThreadOf(ordered, start);
    const isTitle = start.source === TITLE_THREAD_SOURCE && thread !== undefined;
    // A title reply that names the launch thread, or a thread already seen, never becomes a title
    // thread: setting it aside would filter launch traffic out of the judged recording (review F12).
    if (isTitle && (thread === launchThreadId || titleThreads.has(thread)))
      violations.push(
        `title thread/start at ${start.seq} answered with thread ${thread}, which is already the ${thread === launchThreadId ? "launch" : "title"} thread`,
      );
    else if (isTitle) titleThreads.set(thread, start);
    else
      violations.push(
        `thread/start at ${start.seq} (threadSource ${JSON.stringify(start.source)}): an extra thread`,
      );
  }
  return { launchThreadId, titleThreads, violations };
}

/**
 * Every turn/start into the launch thread carries the CLI's client message id: the scripted inputs
 * do, and a Codex-internal turn (such as a title turn sent into the launch thread) does not.
 */
function launchTurnViolations(judged: readonly WireFrame[], launchThreadId: string): string[] {
  return judged.flatMap((frame) => {
    if (frame.direction !== "cliToServer") return [];
    const request = requestSchema.safeParse(frame.message);
    if (!request.success || request.data.method !== "turn/start") return [];
    const params = z
      .looseObject({ threadId: z.string(), clientUserMessageId: z.unknown().optional() })
      .safeParse(request.data.params);
    if (!params.success || params.data.threadId !== launchThreadId) return [];
    const clientId = params.data.clientUserMessageId;
    return typeof clientId === "string" && clientId.length > 0
      ? []
      : [
          `turn/start at ${frame.seq} into the launch thread carries no client message id: a Codex-internal turn, not a scripted input`,
        ];
  });
}

/** What a frame is to the reference's rules: Codex's title traffic, a set-aside history read, or judged. */
type FrameRole = "title" | "read" | "judged";

/**
 * One in-order pass that pairs every reply with the request it answers (review F15, assignment
 * Correction 53). A CLI request stays outstanding, by id on its connection, from its frame until
 * the first serverToCli frame with no `method` and that id; that reply settles it, and the id is
 * free again. Each request frame is classified on its own: Codex's declared title start (by
 * sequence), a request naming a title thread, a set-aside history read naming another thread, or
 * judged. A reply takes its request's role; one that settles no outstanding request is judged. No
 * frame is ever classified by its id alone, so a later request reusing a settled id is judged on
 * what it is, and a server request or a CLI reply sharing an id stays judged (review C49).
 */
function classifyFrames(
  ordered: readonly WireFrame[],
  launchThreadId: string | undefined,
  titleThreads: ReadonlyMap<string, ThreadStart>,
): { readonly roleOf: ReadonlyMap<number, FrameRole>; readonly reads: readonly SetAsideRead[] } {
  const titleStartSeqs = new Set([...titleThreads.values()].map((start) => start.seq));
  const outstanding = new Map<string, FrameRole>();
  const roleOf = new Map<number, FrameRole>();
  const reads: SetAsideRead[] = [];
  for (const frame of ordered) {
    const message = frame.message;
    const isReply =
      typeof message === "object" && message !== null && !("method" in message) && "id" in message;
    if (frame.direction === "cliToServer") {
      const request = requestSchema.safeParse(message);
      // A CLI reply to a server request (no method) is judged.
      if (!request.success) {
        roleOf.set(frame.seq, "judged");
        continue;
      }
      const thread = threadIdSchema.safeParse(request.data.params).data?.threadId;
      const role: FrameRole =
        titleStartSeqs.has(frame.seq) || (thread !== undefined && titleThreads.has(thread))
          ? "title"
          : SET_ASIDE_READ_METHODS.has(request.data.method) &&
              thread !== undefined &&
              thread !== launchThreadId
            ? "read"
            : "judged";
      if (role === "read" && thread !== undefined)
        reads.push({
          seq: frame.seq,
          method: request.data.method,
          threadHash: createHash("sha256").update(thread).digest("hex").slice(0, 16),
        });
      outstanding.set(key(frame.connection, request.data.id), role);
      roleOf.set(frame.seq, role);
      continue;
    }
    if (isReply) {
      const answered = key(frame.connection, replySchema.safeParse(message).data?.id);
      roleOf.set(frame.seq, outstanding.get(answered) ?? "judged");
      outstanding.delete(answered);
      continue;
    }
    // A server request or notification: title traffic only if it names a title thread.
    const thread = namedThread(frame);
    roleOf.set(frame.seq, thread !== undefined && titleThreads.has(thread) ? "title" : "judged");
  }
  return { roleOf, reads };
}

/**
 * Every judged request and notification that names a thread names the launch thread: a turn on
 * any other thread is a violation even when no second thread/start declared it (review F11).
 */
function offThreadViolations(judged: readonly WireFrame[], launchThreadId: string): string[] {
  return judged.flatMap((frame) => {
    const thread = namedThread(frame);
    return thread !== undefined && thread !== launchThreadId
      ? [`frame at ${frame.seq} names thread ${thread}, not the launch thread ${launchThreadId}`]
      : [];
  });
}

/** The launch thread's frames, with Codex's declared title threads set aside. */
export function selectLaunchThread(frames: readonly WireFrame[]): LaunchThreadSelection {
  const ordered = [...frames].sort((left, right) => left.seq - right.seq);
  const { launchThreadId, titleThreads, violations } = classifyStarts(ordered);
  const { roleOf, reads } = classifyFrames(ordered, launchThreadId, titleThreads);
  const judged = ordered.filter((frame) => (roleOf.get(frame.seq) ?? "judged") === "judged");
  return {
    launchThreadId,
    frames: judged,
    violations: [
      ...violations,
      // Without an answered launch start there is no thread to judge frames against; that is
      // already one named violation.
      ...(launchThreadId === undefined
        ? []
        : [
            ...launchTurnViolations(judged, launchThreadId),
            ...offThreadViolations(judged, launchThreadId),
          ]),
    ],
    setAsideReads: reads,
  };
}
