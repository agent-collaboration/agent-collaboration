// Reads the controlled ACP peer's own record of what it received. behavior-agent.ts appends each
// raw `session/prompt` request as one JSON line; in its release-file mode it also appends each
// raw `session/cancel` and a `{kind: "release", sessionId}` row when the release claims the held
// request (Implementer push 01a1a955). So the provider side, not the Router, says how often work
// was dispatched, cancelled or released, and in which order.
import { readFileSync } from "node:fs";
import { z } from "zod";
import { decode } from "../../exact-json.ts";

export interface PeerReceiptCounts {
  readonly prompts: number;
  readonly cancels: number;
  readonly releases: number;
  /** True when no release row precedes this session's first prompt row. */
  readonly releasesFollowPrompt: boolean;
}

const requestRowSchema = z.looseObject({
  method: z.string(),
  params: z.looseObject({ sessionId: z.string() }).optional(),
});
const releaseRowSchema = z.strictObject({ kind: z.literal("release"), sessionId: z.string() });

/** Counts one provider session's rows; other sessions' rows are not this case's. */
export function countPeerRequests(
  receiptText: string,
  providerSessionId: string,
): PeerReceiptCounts {
  let prompts = 0;
  let cancels = 0;
  let releases = 0;
  let releasesFollowPrompt = true;
  for (const line of receiptText.split("\n")) {
    if (line.trim() === "") continue;
    const row = decode(line);
    const release = releaseRowSchema.safeParse(row);
    if (release.success) {
      if (release.data.sessionId !== providerSessionId) continue;
      releases += 1;
      if (prompts === 0) releasesFollowPrompt = false;
      continue;
    }
    const request = requestRowSchema.safeParse(row);
    if (!request.success || request.data.params?.sessionId !== providerSessionId) continue;
    if (request.data.method === "session/prompt") prompts += 1;
    if (request.data.method === "session/cancel") cancels += 1;
  }
  return { prompts, cancels, releases, releasesFollowPrompt };
}

export function readPeerReceipt(path: string, providerSessionId: string): PeerReceiptCounts {
  return countPeerRequests(readFileSync(path, "utf8"), providerSessionId);
}
