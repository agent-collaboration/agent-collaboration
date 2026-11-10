import { test } from "node:test";
import assert from "node:assert/strict";
import { retainProbeCliOutcome, probeCliArguments } from "./staging-isolation-probe.ts";
test("a refused or unconfirmed CLI step keeps exact argv and typed outcome before stopping", async (): Promise<void> => {
  for (const outcome of [
    { kind: "refused", exit: 2, value: { kind: "invalidActor", detail: "literal refusal" } },
    { kind: "unconfirmed", reason: "invalidResponse" },
  ] as const) {
    const saved: { name: string; value: unknown }[] = [];
    await assert.rejects(
      retainProbeCliOutcome({
        arguments: ["workspace", "create", "--actor", "literal actor"],
        outcome,
        retain: async (name, value): Promise<void> => {
          saved.push({ name, value });
        },
      }),
      /typed outcome.*retained/,
    );
    assert.deepEqual(saved, [
      {
        name: "cli-outcome",
        value: { arguments: ["workspace", "create", "--actor", "literal actor"], outcome },
      },
    ]);
  }
});
test("successful CLI evidence remains separate from the typed wrapper and preserves the actual decision", async (): Promise<void> => {
  const saved: { name: string; value: unknown }[] = [];
  const value = { record: { result: { kind: "pendingUnreachable" } } };
  await retainProbeCliOutcome({
    arguments: ["workspace", "create"],
    outcome: { kind: "answered", value },
    retain: async (name, value): Promise<void> => {
      saved.push({ name, value });
    },
  });
  assert.equal(saved[0]?.name, "cli-outcome");
  assert.deepEqual(saved[1], { name: "cli-result", value });
});
test("CLI controls refuse wrapper-owned flags before a producer can run", (): void => {
  const ordinary = ["workspace", "create", "--name", "literal"];
  assert.deepEqual(probeCliArguments(ordinary), ordinary);
  for (const argument of [
    "--json",
    "--json=true",
    "--service-directory",
    "--service-directory=/private/other",
  ])
    assert.throws(() => probeCliArguments([...ordinary, argument]), /wrapper supplies/);
});
