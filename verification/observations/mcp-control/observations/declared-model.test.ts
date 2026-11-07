// The live-offer selection contract, exercised independently of provider processes.
import assert from "node:assert/strict";
import { test } from "node:test";
import { requireSelectedModel } from "./declared-model.ts";

const target = {
  endpoint: { serviceId: "019a0000-0000-7000-8000-000000000001", endpointId: "cursor-local" },
  sessionId: "019a0000-0000-7000-8000-000000000002",
};
const cursor = {
  model: "grok-4.7",
  selectOnCreate: false,
  selectOnRestore: false,
  requiredOptions: { reasoning_effort: "low", fast: "false" },
};
const option = (id: string, value: string): unknown => ({
  id,
  currentValue: { kind: "select", value },
});
const inspect = (options: readonly unknown[]): unknown => ({
  target,
  settingsCatalog: {
    configOptions: [
      { id: "model", category: "model", currentValue: { kind: "select", value: "grok-4.7" } },
      ...options,
    ],
  },
});

test("a Cursor selection accepts separate category-less effort and fast select options", () => {
  assert.doesNotThrow(() =>
    requireSelectedModel(
      inspect([option("reasoning_effort", "low"), option("fast", "false")]),
      target,
      cursor,
    ),
  );
});
for (const [label, options, reason] of [
  [
    "wrong effort",
    [option("reasoning_effort", "high"), option("fast", "false")],
    /required option reasoning_effort/,
  ],
  [
    "fast true",
    [option("reasoning_effort", "low"), option("fast", "true")],
    /required option fast/,
  ],
  ["missing option", [option("reasoning_effort", "low")], /required option fast/],
  [
    "duplicated option",
    [option("reasoning_effort", "low"), option("fast", "false"), option("fast", "false")],
    /required option fast/,
  ],
  [
    "boolean instead of exact select",
    [
      option("reasoning_effort", "low"),
      { id: "fast", currentValue: { kind: "boolean", value: false } },
    ],
    /required option fast/,
  ],
] as const) {
  test(`required selection refuses ${label}`, () => {
    assert.throws(() => requireSelectedModel(inspect(options), target, cursor), reason);
  });
}

test("Claude's haiku catalog alias needs no extra required options", () => {
  assert.doesNotThrow(() =>
    requireSelectedModel(
      {
        target,
        settingsCatalog: {
          configOptions: [
            { id: "model", category: "model", currentValue: { kind: "select", value: "haiku" } },
          ],
        },
      },
      target,
      { model: "haiku", selectOnCreate: true, selectOnRestore: true, requiredOptions: {} },
    ),
  );
});
