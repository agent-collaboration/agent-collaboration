import assert from "node:assert/strict";
import { test } from "node:test";
import { countPeerRequests } from "./peer-receipt.ts";

const prompt = (sessionId: string): string =>
  JSON.stringify({
    jsonrpc: "2.0",
    id: 4,
    method: "session/prompt",
    params: { sessionId, prompt: [] },
  });
const cancel = (sessionId: string): string =>
  JSON.stringify({ jsonrpc: "2.0", method: "session/cancel", params: { sessionId } });
const release = (sessionId: string): string => JSON.stringify({ kind: "release", sessionId });

test("only the case's own session rows are counted, by kind", () => {
  const text = [prompt("s-1"), prompt("s-2"), release("s-1"), cancel("s-2"), ""].join("\n");
  assert.deepEqual(countPeerRequests(text, "s-1"), {
    prompts: 1,
    cancels: 0,
    releases: 1,
    releasesFollowPrompt: true,
  });
});

test("a second prompt and a release before any prompt are both visible", () => {
  const text = [release("s-1"), prompt("s-1"), prompt("s-1"), cancel("s-1")].join("\n");
  assert.deepEqual(countPeerRequests(text, "s-1"), {
    prompts: 2,
    cancels: 1,
    releases: 1,
    releasesFollowPrompt: false,
  });
});
