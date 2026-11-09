/** Version-qualified native API route for an empty persisted recipient, without inference. */
import { setTimeout } from "node:timers/promises";
import { resolve } from "node:path";
import { z } from "zod";
import type { RouterNative } from "../../clients/router-native.ts";

type NativeRpc = Pick<RouterNative, "rpc">;

/** Caller must have created this empty thread in its declared isolated native home. */
export async function persistEmptyRecipient(input: {
  readonly native: NativeRpc;
  readonly threadId: string;
}): Promise<{ archived: unknown; restored: unknown }> {
  const threadId = z.string().min(1).parse(input.threadId);
  const archived = await input.native.rpc("thread/archive", { threadId });
  const restored = await input.native.rpc("thread/unarchive", { threadId });
  z.object({ thread: z.object({ id: z.literal(threadId) }) }).parse(restored);
  return { archived, restored };
}

export async function readUnloadedPersistedRecipient(input: {
  readonly native: NativeRpc;
  readonly threadId: string;
  readonly home: string;
  readonly timeoutMilliseconds?: number;
}): Promise<unknown> {
  const threadId = z.string().min(1).parse(input.threadId);
  if (!input.home.startsWith("/") || resolve(input.home) !== input.home)
    throw new Error("normalized absolute declared native home required");
  const timeout = input.timeoutMilliseconds ?? 15_000;
  if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > 15_000)
    throw new Error("bounded native recipient wait required");
  await input.native.rpc("thread/unsubscribe", { threadId });
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const loaded = z
      .object({ data: z.array(z.string()) })
      .parse(await input.native.rpc("thread/loaded/list", {}));
    if (!loaded.data.includes(threadId)) {
      const stored = await input.native.rpc("thread/read", { threadId, includeTurns: false });
      const view = z
        .object({
          thread: z.object({
            id: z.literal(threadId),
            status: z.object({ type: z.literal("notLoaded") }),
            path: z.string().startsWith(`${input.home}/sessions/`),
            ephemeral: z.literal(false),
          }),
        })
        .parse(stored);
      if (resolve(view.thread.path) !== view.thread.path)
        throw new Error("native recipient path escapes its declared sessions directory");
      return stored;
    }
    await setTimeout(25);
  }
  throw new Error("native recipient did not unload");
}
