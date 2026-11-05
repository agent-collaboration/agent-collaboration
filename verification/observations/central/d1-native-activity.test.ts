import assert from "node:assert/strict";
import { test } from "node:test";
import { nativeTurnStarts } from "./d1-native-activity.ts";

test("D1 no-model observation catches unsolicited native turns even without an explicit turn request", (): void => {
  assert.equal(
    nativeTurnStarts([
      [
        { id: "fixture-reply", result: {} },
        { method: "thread/started", params: {} },
      ],
    ]),
    0,
  );
  assert.equal(
    nativeTurnStarts([
      [
        { method: "turn/started", params: { threadId: "test", turn: { id: "automatic" } } },
        { method: "item/started", params: {} },
        { method: "turn/started", params: { threadId: "test", turn: { id: "second-automatic" } } },
      ],
    ]),
    2,
  );
});

test("a restarted connection cannot erase a turn observed before restart", (): void => {
  assert.equal(
    nativeTurnStarts([
      [{ method: "turn/started", params: { turn: { id: "before-restart" } } }],
      [{ method: "thread/started", params: {} }],
    ]),
    1,
  );
});
