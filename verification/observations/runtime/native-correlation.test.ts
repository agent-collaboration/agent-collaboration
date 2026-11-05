import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";
import { z } from "zod";
import { correlateMessage } from "./native-correlation.ts";

const correlationCases = z
  .array(
    z.object({ name: z.string(), thread: z.unknown(), expected: z.unknown(), result: z.unknown() }),
  )
  .parse(
    JSON.parse(
      readFileSync(new URL("./baselines/native-correlation.json", import.meta.url), "utf8"),
    ),
  );

for (const entry of correlationCases)
  test(`native correlation matches original ${entry.name}`, (): void => {
    assert.deepEqual(correlateMessage(entry.thread, entry.expected), entry.result);
  });

test("native identities are strings and text content cannot be silently omitted", (): void => {
  assert.throws(() =>
    correlateMessage({ id: 123 }, { thread_id: "thread", turn_id: "turn", push_id: "push" }),
  );
  assert.throws(
    () =>
      correlateMessage(
        {
          id: "thread",
          turns: [
            {
              id: "turn",
              items: [
                { id: "item", type: "userMessage", clientId: "push", content: [{ type: "text" }] },
              ],
            },
          ],
        },
        { thread_id: "thread", turn_id: "turn", push_id: "push" },
      ),
    /native text content missing/,
  );
});
