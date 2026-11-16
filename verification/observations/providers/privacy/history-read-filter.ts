/**
 * The live-recording privacy filter (D0 R42, Program Design Y3 "Privacy, before anything is
 * written"). A recorder knows the test session's thread ids. A Codex history read of any other
 * thread (`thread/read`, `thread/turns/list`, `thread/items/list`, `thread/list`,
 * `thread/search`) is dropped together with its reply, matched by connection and JSON-RPC id, and
 * only the method and a hash of the thread id are kept. `thread/list` and `thread/search` can
 * return other threads whatever their params say, so they are always dropped.
 *
 * Frames that cannot be correlated are treated safely: a reply whose request this filter never
 * saw on that connection is dropped too, because its body could be another thread's history.
 */
import { createHash } from "node:crypto";

export const HISTORY_READ_METHODS = [
  "thread/read",
  "thread/turns/list",
  "thread/items/list",
  "thread/list",
  "thread/search",
] as const;

/** Reads that list or search across threads: never kept, whatever their params name. */
const CROSS_THREAD_METHODS: ReadonlySet<string> = new Set(["thread/list", "thread/search"]);

export type FilterVerdict =
  | { readonly kind: "keep" }
  | {
      readonly kind: "redact";
      readonly redacted: { readonly method: string; readonly threadIdHash: string | null };
    };

function hashThreadId(threadId: unknown): string | null {
  return typeof threadId === "string"
    ? createHash("sha256").update(threadId).digest("hex").slice(0, 16)
    : null;
}

function objectOf(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

/** One filter per recording: it remembers requests per connection and direction. */
export class HistoryReadFilter {
  private readonly ownThreads: ReadonlySet<string>;
  /** `${connection}\u0000${sender}\u0000${id}` of every request seen, and whether it was dropped. */
  private readonly requests = new Map<string, FilterVerdict>();

  constructor(ownThreadIds: readonly string[]) {
    this.ownThreads = new Set(ownThreadIds);
  }

  /**
   * Judges one frame `sender` sent on `connection`. Requests are remembered so their replies,
   * travelling the other way, share their verdict.
   */
  judge(connection: string, sender: "client" | "server", message: unknown): FilterVerdict {
    const frame = objectOf(message);
    if (frame === undefined) return { kind: "keep" };
    const id = frame["id"];
    const method = typeof frame["method"] === "string" ? frame["method"] : undefined;
    const key = (from: string): string => `${connection}\u0000${from}\u0000${JSON.stringify(id)}`;
    if (method !== undefined) {
      const verdict = this.requestVerdict(method, objectOf(frame["params"]));
      if (id !== undefined) this.requests.set(key(sender), verdict);
      return verdict;
    }
    if (id === undefined) return { kind: "keep" };
    const requester = sender === "client" ? "server" : "client";
    const request = this.requests.get(key(requester));
    if (request === undefined)
      return { kind: "redact", redacted: { method: "uncorrelatedReply", threadIdHash: null } };
    this.requests.delete(key(requester));
    return request;
  }

  private requestVerdict(
    method: string,
    params: Record<string, unknown> | undefined,
  ): FilterVerdict {
    if (!(HISTORY_READ_METHODS as readonly string[]).includes(method)) return { kind: "keep" };
    const threadId = params?.["threadId"];
    const ownThread = typeof threadId === "string" && this.ownThreads.has(threadId);
    if (ownThread && !CROSS_THREAD_METHODS.has(method)) return { kind: "keep" };
    return { kind: "redact", redacted: { method, threadIdHash: hashThreadId(threadId) } };
  }
}
