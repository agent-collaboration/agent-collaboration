/**
 * `node verification/observations/providers/mcp/mcp-live-record.ts <plan.json>`
 *
 * The live MCP recording (D0 Program Design Y3): this script is the MCP caller of the debug
 * Host's endpoint and keeps its own exchange, exactly as it crossed: `initialize`,
 * `notifications/initialized`, `tools/list`, then each planned tool call. The N11 tap is
 * test-only and plays no part live. The recording is JSON lines `{seq, connection, direction,
 * message}`, the format `check_recording` reads (`cargo run -p acp-provider-adapters --features test-support --example check_recording -- mcp <file>`).
 *
 * Plan (JSON):
 *   { "endpoint": "http://127.0.0.1:<port>/mcp", "recordingPath": "<absolute .jsonl>",
 *     "sessionModel": "claude-haiku-5-5" | "gpt-6-luna",
 *     "steps": [{ "name": "sessionCommand" | "acpCarrier" | "nativeControl",
 *                 "tool": "<advertised tool>", "arguments": { ... } }] }
 *
 * Before the first message: the endpoint must be isolated loopback (never the production ports),
 * and the session the calls drive must run Haiku or Luna; otherwise nothing is sent. Each step's
 * prerequisite is its tool being advertised: a step whose tool `tools/list` did not advertise is
 * reported "not exercised" in `<recording>.steps.json`, never skipped silently or faked.
 */
import { appendFileSync, existsSync, writeFileSync } from "node:fs";
import { readFileSync } from "node:fs";
import { isAbsolute } from "node:path";

import { z } from "zod";

import { runtimeClock } from "../../runtime/runtime-plan.ts";

export const ALLOWED_SESSION_MODELS = ["claude-haiku-5-5", "gpt-6-luna"] as const;
const PRODUCTION_PORTS: readonly number[] = [8787, 8788, 19741, 19742, 19743, 43127];

const planSchema = z.strictObject({
  endpoint: z.url(),
  recordingPath: z.string().refine(isAbsolute, "recordingPath must be absolute"),
  sessionModel: z.enum(ALLOWED_SESSION_MODELS),
  steps: z
    .array(
      z.strictObject({
        name: z.enum(["sessionCommand", "acpCarrier", "nativeControl"]),
        tool: z.string().min(1),
        arguments: z.record(z.string(), z.unknown()),
      }),
    )
    .min(1),
});
export type LiveMcpPlan = z.infer<typeof planSchema>;

export type StepOutcome =
  | {
      readonly name: string;
      readonly tool: string;
      readonly kind: "exercised";
      readonly frames: number;
    }
  | {
      readonly name: string;
      readonly tool: string;
      readonly kind: "notExercised";
      readonly reason: string;
    };

/** Checks a plan before anything is sent; throws naming the first problem. */
export function parsePlan(value: unknown): LiveMcpPlan {
  const plan = planSchema.parse(value);
  const endpoint = new URL(plan.endpoint);
  if (
    endpoint.protocol !== "http:" ||
    endpoint.hostname !== "127.0.0.1" ||
    endpoint.username !== "" ||
    endpoint.password !== "" ||
    PRODUCTION_PORTS.includes(Number(endpoint.port))
  )
    throw new Error("an isolated loopback debug MCP endpoint is required");
  if (existsSync(plan.recordingPath))
    throw new Error("recordingPath already exists; choose a fresh path");
  return plan;
}

/** The JSON-RPC frames of one HTTP reply: a JSON body, or the `data:` events of an SSE stream. */
export function replyFrames(contentType: string, body: string): unknown[] {
  if (!contentType.includes("text/event-stream"))
    return body.trim() === "" ? [] : [JSON.parse(body)];
  return body.split(/\r?\n\r?\n/u).flatMap((event): unknown[] => {
    const data = event
      .split(/\r?\n/u)
      .filter((line) => line.startsWith("data:"))
      .map((line) => line.slice(5).trimStart())
      .join("\n");
    return data === "" ? [] : [JSON.parse(data)];
  });
}

class Recorder {
  private seq = 0;
  private readonly path: string;
  constructor(path: string) {
    this.path = path;
    writeFileSync(path, "", { flag: "wx" });
  }
  record(direction: "clientToServer" | "serverToClient", message: unknown): void {
    appendFileSync(
      this.path,
      `${JSON.stringify({ seq: this.seq, connection: 0, direction, message })}\n`,
    );
    this.seq += 1;
  }
}

/** Sends one frame, records it and every frame of the reply, and returns the reply frames. */
/** The one clock every wait here takes its time from (D0 R43): it makes each request's deadline. */
export interface LiveClock {
  /**
   * A deadline `milliseconds` away on this clock: its signal aborts when it passes, and
   * `release` lets it go once the request has finished either way.
   */
  deadline(milliseconds: number): { readonly signal: AbortSignal; readonly release: () => void };
}

/** The production clock: the runtime's one clock, made in runtime-plan.ts. */
export const liveClock: LiveClock = {
  deadline: (milliseconds) =>
    runtimeClock.deadline(milliseconds, `MCP request timed out after ${milliseconds} ms`),
};

/** How long one MCP request may take before the run stops. */
export const REQUEST_DEADLINE_MS = 120_000;

async function exchange(
  plan: LiveMcpPlan,
  recorder: Recorder,
  clock: LiveClock,
  message: object,
): Promise<unknown[]> {
  recorder.record("clientToServer", message);
  const deadline = clock.deadline(REQUEST_DEADLINE_MS);
  try {
    const reply = await fetch(plan.endpoint, {
      method: "POST",
      redirect: "error",
      signal: deadline.signal,
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json, text/event-stream",
        "MCP-Protocol-Version": "2025-11-25",
      },
      body: JSON.stringify(message),
    });
    const frames = replyFrames(reply.headers.get("content-type") ?? "", await reply.text());
    for (const frame of frames) recorder.record("serverToClient", frame);
    if (!reply.ok) throw new Error(`MCP HTTP ${reply.status} for ${JSON.stringify(message)}`);
    return frames;
  } finally {
    deadline.release();
  }
}

const toolsListSchema = z.looseObject({
  result: z.looseObject({ tools: z.array(z.looseObject({ name: z.string() })) }),
});

export async function recordLiveMcp(
  plan: LiveMcpPlan,
  clock: LiveClock = liveClock,
): Promise<StepOutcome[]> {
  const recorder = new Recorder(plan.recordingPath);
  let id = 0;
  await exchange(plan, recorder, clock, {
    jsonrpc: "2.0",
    id: id++,
    method: "initialize",
    params: {
      protocolVersion: "2025-11-25",
      capabilities: {},
      clientInfo: { name: "d0-mcp-live-record", version: "1" },
    },
  });
  await exchange(plan, recorder, clock, { jsonrpc: "2.0", method: "notifications/initialized" });
  const listed = await exchange(plan, recorder, clock, {
    jsonrpc: "2.0",
    id: id++,
    method: "tools/list",
    params: {},
  });
  const advertised = new Set(
    listed.flatMap((frame) => {
      const parsed = toolsListSchema.safeParse(frame);
      return parsed.success ? parsed.data.result.tools.map((tool) => tool.name) : [];
    }),
  );
  const outcomes: StepOutcome[] = [];
  for (const step of plan.steps) {
    if (!advertised.has(step.tool)) {
      outcomes.push({
        name: step.name,
        tool: step.tool,
        kind: "notExercised",
        reason: "tool not advertised by tools/list",
      });
      continue;
    }
    const frames = await exchange(plan, recorder, clock, {
      jsonrpc: "2.0",
      id: id++,
      method: "tools/call",
      params: { name: step.tool, arguments: step.arguments },
    });
    outcomes.push({ name: step.name, tool: step.tool, kind: "exercised", frames: frames.length });
  }
  writeFileSync(`${plan.recordingPath}.steps.json`, `${JSON.stringify(outcomes, null, 2)}\n`);
  return outcomes;
}

if (import.meta.main) {
  const [planPath] = process.argv.slice(2);
  if (planPath === undefined) {
    console.error(
      "usage: node verification/observations/providers/mcp/mcp-live-record.ts <plan.json>",
    );
    process.exitCode = 2;
  } else {
    const plan = parsePlan(JSON.parse(readFileSync(planPath, "utf8")));
    const outcomes = await recordLiveMcp(plan);
    for (const outcome of outcomes) console.log(`${outcome.name} ${outcome.tool}: ${outcome.kind}`);
  }
}
