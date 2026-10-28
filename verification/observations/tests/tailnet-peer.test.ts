import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { readObservedJson } from "../evidence-models.ts";
import { boardPlanSchema, requireDeadline } from "../tailnet-plans.ts";
import { runPeer, type PeerRequest } from "../tailnet-board-peer.ts";

const directory = new URL("baselines/", import.meta.url);
const callSchema = z.object({
  target: z.unknown(),
  payload: z.unknown(),
  headers: z.unknown(),
  reply: z
    .object({ status: z.number().or(z.bigint()).transform(Number), bodyBase64: z.string() })
    .optional(),
  failure: z.object({ category: z.string(), message: z.string() }).optional(),
});
const baselineSchema = z.object({
  mode: z.string(),
  plan: z.unknown(),
  calls: z.array(callSchema),
  expected: z.object({
    refused: z.boolean(),
    replayRefused: z.boolean(),
    posts: z.number().or(z.bigint()).transform(Number),
    files: z.unknown(),
  }),
});
const headersSchema = z.object({
  origin: z.string().nullable().default(null),
  host_authority: z.string().nullable().default(null),
});
function replaceRoot(value: unknown, from: string, to: string): unknown {
  if (typeof value === "string") return value.replaceAll(from, to);
  if (Array.isArray(value)) return value.map((item: unknown) => replaceRoot(item, from, to));
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([name, item]) => [name, replaceRoot(item, from, to)]),
    );
  return value;
}

for (const filename of readdirSync(directory)
  .filter((name) => name.startsWith("tailnet-peer-") && name.endsWith(".bin"))
  .sort()) {
  const baseline = baselineSchema.parse(
    decodeEvidenceJson(readFileSync(new URL(filename, directory))),
  );
  test(`tailnet peer matches original post count, raw evidence and no replay: ${baseline.mode}`, async (): Promise<void> => {
    const root = createFixtureDirectory("tailnet-peer");
    const plan = boardPlanSchema.parse(replaceRoot(baseline.plan, "<ROOT>", root));
    let cursor = 0;
    let posts = 0;
    const request: PeerRequest = async (target, payload, headers): ReturnType<PeerRequest> => {
      const call = baseline.calls[cursor++];
      assert.notEqual(call, undefined, "unexpected additional request");
      if (call === undefined) throw new Error("missing original exchange");
      assert.deepEqual(target, { kind: "https", endpoint: call.target });
      assert.deepEqual(payload, call.payload);
      assert.deepEqual(
        headers === undefined || headers === null ? null : headersSchema.parse(headers),
        call.headers,
      );
      const parameters = z
        .object({ name: z.string(), arguments: z.unknown() })
        .safeParse(payload.params);
      if (parameters.success && parameters.data.name === "board_message_post") {
        posts++;
        assert.deepEqual(
          readObservedJson(readFileSync(join(root, "peer-intent.json"))).postArguments,
          parameters.data.arguments,
        );
      }
      if (call.failure !== undefined) {
        const error = new Error(call.failure.message);
        error.name = call.failure.category;
        throw error;
      }
      if (call.reply === undefined) throw new Error("original reply absent");
      return { status: call.reply.status, body: Buffer.from(call.reply.bodyBase64, "base64") };
    };
    const checkAllocation = (deadline: string): void => {
      requireDeadline(
        baseline.mode === "expired-after-preflight" && existsSync(join(root, "peer-intent.json"))
          ? "2000-01-01T00:00:00Z"
          : deadline,
      );
    };
    try {
      let refused = false;
      try {
        await runPeer(plan, request, checkAllocation);
      } catch {
        refused = true;
      }
      assert.equal(cursor, baseline.calls.length);
      assert.equal(refused, baseline.expected.refused);
      let replayRefused = false;
      if (baseline.mode !== "wrong-root" && baseline.mode !== "expired-after-preflight") {
        cursor = 0;
        await assert.rejects(
          () => runPeer(plan, request, checkAllocation),
          (error: unknown): boolean => {
            replayRefused = z.object({ code: z.literal("EEXIST") }).safeParse(error).success;
            return replayRefused;
          },
        );
      }
      const files = Object.fromEntries(
        readdirSync(root)
          .filter((name) => name.endsWith(".json"))
          .sort()
          .map((name) => [name, decodeEvidenceJson(readFileSync(join(root, name)))]),
      );
      assert.deepEqual(
        { refused, replayRefused, posts, files: replaceRoot(files, root, "<ROOT>") },
        baseline.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
