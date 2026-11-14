import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { objectSchema } from "../exact-json.ts";

export function frameMetadata(
  frame: Readonly<Record<string, unknown>>,
): Readonly<Record<string, unknown>> {
  const params = objectSchema.safeParse(frame.params);
  const sessionId = params.success ? params.data.sessionId : undefined;
  const update = objectSchema.safeParse(params.success ? params.data.update : undefined);
  const error = objectSchema.safeParse(frame.error);
  return {
    id:
      typeof frame.id === "string" && /^[a-z][a-z0-9-]{0,40}$/.test(frame.id)
        ? frame.id
        : typeof frame.id,
    method: typeof frame.method === "string" ? frame.method.slice(0, 64) : undefined,
    sessionId:
      typeof sessionId === "string" && /^session-[0-9]+$/.test(sessionId) ? sessionId : undefined,
    update: String((update.success ? update.data.sessionUpdate : undefined) ?? "absent").slice(
      0,
      64,
    ),
    errorCode: String((error.success ? error.data.code : undefined) ?? "absent").slice(0, 32),
  };
}

export function reportPeerPhase(props: {
  reason: string;
  receipt: string;
  pendingPhase: string;
  pendingRequest: Readonly<Record<string, unknown>> | undefined;
  frames: readonly Record<string, unknown>[];
  exitCode: number | null;
  signalCode: NodeJS.Signals | null;
  stderr: string;
}): void {
  const diagnostics = props.receipt + ".release-diagnostics.jsonl";
  process.stderr.write(
    JSON.stringify({
      fixture: "behavior-peer",
      reason: props.reason,
      pendingPhase: props.pendingPhase,
      pendingRequest: props.pendingRequest,
      frameCount: props.frames.length,
      frames: props.frames.slice(-16).map(frameMetadata),
      exitCode: props.exitCode,
      signalCode: props.signalCode,
      stderrBytes: Buffer.byteLength(props.stderr),
      stderrSha256: createHash("sha256").update(props.stderr).digest("hex"),
      releaseDiagnostics:
        process.env.BEHAVIOR_PEER_RELEASE_DIAGNOSTICS === "1" && existsSync(diagnostics)
          ? readFileSync(diagnostics, "utf8")
          : undefined,
    }) + "\n",
  );
}
export function spawnBehaviorPeer(props: {
  receipt: string;
  profile: "lifecycle" | "manual";
  authentication: boolean;
  release: boolean;
  releaseSocket: string;
  releasePoolContention: boolean;
}): ChildProcessWithoutNullStreams {
  return spawn(
    process.execPath,
    [
      new URL("../../crates/acp-client-runtime/tests/support/behavior-agent.ts", import.meta.url)
        .pathname,
      props.receipt,
      props.profile,
      ...(props.authentication ? ["cursor_login"] : []),
      ...(props.release ? [props.releaseSocket] : []),
    ],
    {
      env: props.releasePoolContention
        ? {
            ...process.env,
            UV_THREADPOOL_SIZE: "1",
            BEHAVIOR_PEER_RELEASE_POOL_CONTENTION: "1",
          }
        : process.env,
    },
  );
}
