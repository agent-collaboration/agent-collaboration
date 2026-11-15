import assert from "node:assert/strict";
import { test } from "node:test";
import { captureNativeSnapshot } from "./native-observation.ts";

const target = {
  endpoint: { serviceId: "service-a", endpointId: "codex-local" },
  sessionId: "fresh-native",
};
function inspect(turns: unknown[] = []): unknown {
  return {
    target,
    effectiveAccess: "workspace-write",
    thread: {
      id: target.sessionId,
      model: "gpt-6-luna",
      reasoningEffort: "low",
      status: { type: "idle" },
      turns,
    },
  };
}

test("a fresh native baseline needs no rollout or event attachment", async (): Promise<void> => {
  const calls: string[] = [];
  const result = await captureNativeSnapshot({
    target,
    expectedModel: "gpt-6-luna",
    call: async (name, args): Promise<unknown> => {
      calls.push(name);
      if (name === "events_observe") throw new Error("no rollout found");
      assert.equal(name, "session_inspect");
      assert.deepEqual(args, { target });
      return inspect();
    },
  });
  assert.deepEqual(calls, ["session_inspect"]);
  assert.match(JSON.stringify(result), /nativeThreadSnapshot/);
  assert.match(JSON.stringify(result), /"eventStreamAttached":false/);
  assert.match(JSON.stringify(result), /"turnsRead":false/);
  assert.match(JSON.stringify(result), /"turns":\[\]/);
});

test("native action readback keeps shell outcome but removes model thoughts and message bodies", async (): Promise<void> => {
  const result = await captureNativeSnapshot({
    target,
    expectedModel: "gpt-6-luna",
    call: async (): Promise<unknown> => inspect(),
    readTurns: async (): Promise<unknown> => ({
      thread: {
        id: target.sessionId,
        status: { type: "idle" },
        turns: [
          {
            id: "turn-a",
            status: "completed",
            items: [
              { id: "thought", type: "reasoning", text: "PRIVATE_THOUGHT" },
              { id: "reply", type: "agentMessage", text: "PRIVATE_BODY" },
              {
                id: "shell",
                type: "commandExecution",
                command: "echo $TMPDIR",
                cwd: "/workspace",
                exitCode: 0,
                aggregatedOutput: "/scratch/session-a\n",
                status: "completed",
              },
            ],
          },
        ],
      },
    }),
  });
  const serialized = JSON.stringify(result);
  assert.match(serialized, /scratch\/session-a/);
  assert.match(serialized, /"exitCode":0/);
  assert.equal(serialized.includes("PRIVATE_THOUGHT"), false);
  assert.equal(serialized.includes("PRIVATE_BODY"), false);
});

test("native baseline refuses wrong identity and changed model rather than masking them", async (): Promise<void> => {
  await assert.rejects(
    captureNativeSnapshot({
      target,
      expectedModel: "different-model",
      call: async (): Promise<unknown> => inspect(),
    }),
    /model\/effort/,
  );
  await assert.rejects(
    captureNativeSnapshot({
      target: { ...target, sessionId: "wrong" },
      expectedModel: "gpt-6-luna",
      call: async (): Promise<unknown> => inspect(),
    }),
    /target mismatch/,
  );
  await assert.rejects(
    captureNativeSnapshot({
      target,
      expectedModel: "gpt-6-luna",
      call: async (): Promise<unknown> => inspect(),
      readTurns: async (): Promise<unknown> => ({
        thread: { id: "another-thread", status: { type: "idle" }, turns: [] },
      }),
    }),
    /turn read target mismatch/,
  );
  await assert.rejects(
    captureNativeSnapshot({
      target,
      expectedModel: "gpt-6-luna",
      call: async (): Promise<unknown> => inspect(),
      readTurns: async (): Promise<unknown> => {
        throw new Error("native unavailable");
      },
    }),
    /native unavailable/,
  );
});
