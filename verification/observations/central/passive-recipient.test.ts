import assert from "node:assert/strict";
import { test } from "node:test";
import { persistEmptyRecipient, readUnloadedPersistedRecipient } from "./passive-recipient.ts";

const threadId = "01900000-0000-7000-8000-000000000001";
const home = "/declared/fixture/codex";
const stored = {
  thread: {
    id: threadId,
    status: { type: "notLoaded" },
    ephemeral: false,
    path: `${home}/sessions/empty.jsonl`,
  },
};

test("empty persistence calls only the archive/unarchive API on the exact thread", async (): Promise<void> => {
  const calls: { method: string; params: unknown }[] = [];
  const native = {
    rpc: async (method: string, params: unknown): Promise<unknown> => {
      calls.push({ method, params });
      if (method === "thread/archive") return {};
      if (method === "thread/unarchive") return stored;
      throw new Error("a model or unrelated method was invoked");
    },
  };
  assert.deepEqual(await persistEmptyRecipient({ native, threadId }), {
    archived: {},
    restored: stored,
  });
  assert.deepEqual(calls, [
    { method: "thread/archive", params: { threadId } },
    { method: "thread/unarchive", params: { threadId } },
  ]);
  await assert.rejects(
    persistEmptyRecipient({
      native: { rpc: async (): Promise<unknown> => ({ thread: { id: "wrong" } }) },
      threadId,
    }),
  );
});

test("unloaded persistence requires the exact non-ephemeral record in its declared home", async (): Promise<void> => {
  const methods: string[] = [];
  const native = {
    rpc: async (method: string): Promise<unknown> => {
      methods.push(method);
      if (method === "thread/unsubscribe") return {};
      if (method === "thread/loaded/list") return { data: [] };
      if (method === "thread/read") return stored;
      throw new Error("unexpected native operation");
    },
  };
  assert.deepEqual(await readUnloadedPersistedRecipient({ native, threadId, home }), stored);
  assert.deepEqual(methods, ["thread/unsubscribe", "thread/loaded/list", "thread/read"]);
  for (const change of [
    { ephemeral: true },
    { id: "other" },
    { status: { type: "idle" } },
    { path: "/another/home/sessions/empty.jsonl" },
    { path: `${home}/sessions/../outside.jsonl` },
  ])
    await assert.rejects(
      readUnloadedPersistedRecipient({
        native: {
          rpc: async (method: string): Promise<unknown> =>
            method === "thread/read" ? { thread: { ...stored.thread, ...change } } : { data: [] },
        },
        threadId,
        home,
      }),
    );
});

test("loaded-only or malformed recipients never become persisted fixture proof", async (): Promise<void> => {
  const native = { rpc: async (): Promise<unknown> => ({ data: [threadId] }) };
  await assert.rejects(
    readUnloadedPersistedRecipient({ native, threadId, home, timeoutMilliseconds: 1 }),
    /did not unload/,
  );
  await assert.rejects(
    readUnloadedPersistedRecipient({ native, threadId, home: "/declared/../elsewhere" }),
    /normalized absolute/,
  );
  await assert.rejects(
    readUnloadedPersistedRecipient({ native, threadId, home, timeoutMilliseconds: 0 }),
    /bounded/,
  );
});
