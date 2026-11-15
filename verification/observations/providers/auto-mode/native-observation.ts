// Native thread snapshots work before a rollout exists. They do not establish
// event continuity; do not attach events_observe as a prerequisite for first input.
import { z } from "zod";
import { lstatSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join } from "node:path";
import { randomUUIDv7 } from "node:crypto";
import { UnixWebSocketObserver } from "../acp/unix-websocket-observer.ts";
import { decode, encode } from "../../../exact-json.ts";
import {
  sameSession,
  sessionRefSchema,
} from "../../mcp-control/observations/observation-models.ts";

const record = z.record(z.string(), z.unknown());

export async function captureNativeSnapshot(input: {
  readonly target: z.infer<typeof sessionRefSchema>;
  readonly expectedModel: string;
  readonly call: (name: string, arguments_: Readonly<Record<string, unknown>>) => Promise<unknown>;
  readonly readTurns?: () => Promise<unknown>;
}): Promise<unknown> {
  const value = z
    .object({
      target: sessionRefSchema,
      generation: z.unknown().optional(),
      effectiveAccess: z.string().nullish(),
      settingsObservation: z.unknown().optional(),
      thread: z.object({
        id: z.string(),
        model: z.string(),
        reasoningEffort: z.string(),
        status: z.unknown(),
        turns: z.array(record).default([]),
      }),
    })
    .parse(await input.call("session_inspect", { target: input.target }));
  if (!sameSession(value.target, input.target) || value.thread.id !== input.target.sessionId)
    throw new Error("native snapshot target mismatch");
  if (value.thread.model !== input.expectedModel || value.thread.reasoningEffort !== "low")
    throw new Error("native snapshot model/effort mismatch");
  // Public inspection deliberately requests includeTurns=false. Only the after
  // snapshot reads turns from the allocated native socket, once input has a rollout.
  const thread =
    input.readTurns === undefined
      ? value.thread
      : z
          .object({
            id: z.string(),
            status: z.unknown(),
            turns: z.array(record),
          })
          .parse(z.object({ thread: z.unknown() }).parse(await input.readTurns()).thread);
  if (thread.id !== input.target.sessionId) throw new Error("native turn read target mismatch");
  return {
    target: value.target,
    generation: value.generation,
    effectiveAccess: value.effectiveAccess,
    settingsObservation: value.settingsObservation,
    coverage: {
      kind: "nativeThreadSnapshot",
      eventStreamAttached: false,
      turnsRead: input.readTurns !== undefined,
      reason:
        "Fresh native threads can refuse event attachment before their first rollout; snapshots do not prove interval continuity.",
    },
    thread: {
      ...value.thread,
      status: thread.status,
      turns: thread.turns.map((turn): unknown => ({
        id: turn["id"],
        status: turn["status"],
        items: z
          .array(record)
          .parse(turn["items"] ?? [])
          .map((item): unknown => ({
            id: item["id"],
            type: item["type"],
            status: item["status"],
            ...(item["type"] === "commandExecution"
              ? {
                  command: item["command"],
                  cwd: item["cwd"],
                  exitCode: item["exitCode"],
                  aggregatedOutput:
                    typeof item["aggregatedOutput"] === "string"
                      ? item["aggregatedOutput"].slice(0, 4096)
                      : undefined,
                  outputTruncated:
                    typeof item["aggregatedOutput"] === "string" &&
                    item["aggregatedOutput"].length > 4096,
                }
              : {}),
          })),
      })),
    },
  };
}

/** Read-only metadata request through this allocation's owner-private native socket. */
export async function readNativeThreadTurns(
  runtimePlan: string,
  target: z.infer<typeof sessionRefSchema>,
): Promise<unknown> {
  const socket = z
    .object({ directory: z.string().refine(isAbsolute), mode: z.literal("0700") })
    .parse(JSON.parse(readFileSync(join(dirname(runtimePlan), "socket-directory.json"), "utf8")));
  const status = lstatSync(socket.directory);
  if (
    !status.isDirectory() ||
    status.isSymbolicLink() ||
    status.uid !== process.getuid?.() ||
    (status.mode & 0o077) !== 0
  )
    throw new Error("native snapshot socket ownership mismatch");
  const client = new UnixWebSocketObserver(join(socket.directory, "app.sock"));
  try {
    await client.opened();
    async function request(method: string, params: unknown): Promise<unknown> {
      const id = randomUUIDv7();
      await client.send(encode({ jsonrpc: "2.0", id, method, params }));
      for (;;) {
        const frame = record.parse(decode(await client.receive()));
        if (frame["id"] !== id) continue;
        if (frame["error"] !== undefined) throw new Error(`native snapshot ${method} refused`);
        return frame["result"];
      }
    }
    await request("initialize", {
      clientInfo: { name: "d1-native-snapshot", version: "1" },
      capabilities: { experimentalApi: true },
    });
    await client.send(encode({ jsonrpc: "2.0", method: "initialized", params: {} }));
    return await request("thread/read", { threadId: target.sessionId, includeTurns: true });
  } finally {
    await client.close();
  }
}
