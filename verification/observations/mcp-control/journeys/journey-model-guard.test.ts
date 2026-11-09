// Real case wiring and MCP reads against the existing stand-in; no real provider input.
import assert from "node:assert/strict";
import { decode } from "../../../exact-json.ts";
import { z } from "zod";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import {
  startStandIn,
  STAND_IN_SERVICE,
  STAND_IN_OWNER,
  type StandInOptions,
} from "../test-support/journey-stand-in.ts";
import { standInPlan } from "../test-support/journey-stand-in-plan.ts";
import { ProviderStoreReadback } from "../evidence/role-readback.ts";
import { StreamingMcpClient } from "../transport/streaming-mcp-client.ts";
import { runCases } from "../run.ts";
import type { CaseResult } from "./journey-cases.ts";
import type { DeclaredProviderSelection } from "../observations/declared-model.ts";

const MODEL = "haiku";
const CLAUDE_SELECTION = {
  model: MODEL,
  selectOnCreate: true,
  selectOnRestore: true,
  requiredOptions: {},
};
const CURSOR_SELECTION = {
  model: "grok-4.7",
  selectOnCreate: false,
  selectOnRestore: false,
  requiredOptions: { reasoning_effort: "low", fast: "false" },
};

async function runModelCase(
  options: StandInOptions,
  selection: DeclaredProviderSelection = CLAUDE_SELECTION,
): Promise<{
  readonly result: CaseResult;
  readonly calls: readonly string[];
  readonly createModel: unknown;
  readonly createModelPresent: boolean;
  readonly evidence: string;
  readonly settingsRequests: readonly Readonly<Record<string, unknown>>[];
}> {
  const standIn = await startStandIn(options);
  try {
    const plan = standInPlan(standIn, ["loadResume"], { peer: { kind: "provider", selection } });
    mkdirSync(plan.evidence);
    const store = new ProviderStoreReadback(standIn.store);
    try {
      const [result] = await runCases(
        plan,
        {
          client: new StreamingMcpClient(standIn.url),
          evidence: plan.evidence,
          deadline: Date.parse(plan.deadline),
        },
        store,
      );
      if (result === undefined) throw new Error("missing result");
      return {
        result,
        calls: [...standIn.calls],
        createModel: standIn.requests.find((request) => request.name === "conversation_create")
          ?.arguments["model"],
        createModelPresent: Object.hasOwn(
          standIn.requests.find((request) => request.name === "conversation_create")?.arguments ??
            {},
          "model",
        ),
        settingsRequests: standIn.requests
          .filter((request) => request.name === "conversation_settings_set")
          .map((request) => request.arguments),
        evidence: readFileSync(join(plan.evidence, "exchanges.jsonl"), "utf8"),
      };
    } finally {
      store.close();
    }
  } finally {
    await standIn.close();
  }
}

test("provider admission requires a selection before any MCP effect; controlled plans stay unchanged", async () => {
  const standIn = await startStandIn();
  try {
    assert.throws(() => standInPlan(standIn, ["lifecycle"], { peer: { kind: "provider" } }));
    const { selectOnRestore: _restoreFlag, ...legacySelection } = CLAUDE_SELECTION;
    assert.throws(() =>
      standInPlan(standIn, ["lifecycle"], {
        peer: { kind: "provider", selection: legacySelection },
      }),
    );
    const controlled = standInPlan(standIn, ["lifecycle"]);
    assert.equal(controlled.peer.kind, "controlled");
    assert.equal("selection" in controlled.peer, false);
    assert.deepEqual(standIn.calls, []);
  } finally {
    await standIn.close();
  }
});

test("provider create sends its declared model and every prompt has a recorded inspect", async () => {
  const observed = await runModelCase({});
  assert.equal(observed.result.verdict.kind, "qualified");
  assert.equal(observed.createModel, MODEL);
  assert.equal(observed.calls.filter((name) => name === "provider_session_inspect").length, 5);
  for (const [index, name] of observed.calls.entries()) {
    if (name === "conversation_prompt")
      assert.equal(observed.calls[index - 1], "provider_session_inspect");
  }
  assert.equal(observed.evidence.split('"tool":"provider_session_inspect"').length - 1, 5);
});

for (const [label, options, prompts, reason] of [
  ["notApplied", { createNotApplied: true }, 0, "creation did not apply all declared settings"],
  [
    "createdWithoutSettings",
    { createdWithoutSettings: true },
    0,
    "creation did not apply all declared settings",
  ],
  [
    "model changed after resume",
    { modelAfterResume: "other-model" },
    1,
    "current model other-model differs from declared model haiku",
  ],
  [
    "model changed after load",
    { modelAfterLoad: "other-model" },
    2,
    "current model other-model differs from declared model haiku",
  ],
  [
    "wrong initial model",
    { currentModel: "other-model" },
    0,
    "current model other-model differs from declared model haiku",
  ],
] as const) {
  test(`provider stops before input: ${label}`, async () => {
    const observed = await runModelCase(options, { ...CLAUDE_SELECTION, selectOnRestore: false });
    assert.equal(observed.result.verdict.kind, "failed");
    if (observed.result.verdict.kind !== "failed") throw new Error("expected failure");
    assert.equal(
      observed.result.verdict.reason.includes(reason),
      true,
      observed.result.verdict.reason,
    );
    assert.equal(observed.calls.filter((name) => name === "conversation_prompt").length, prompts);
    if (label === "notApplied" || label === "createdWithoutSettings") {
      assert.deepEqual(observed.result.target, {
        endpoint: { serviceId: STAND_IN_SERVICE, endpointId: "cursor-local" },
        sessionId: "session-1",
      });
      assert.deepEqual(observed.calls, ["conversation_create"]);
    }
  });
}

const requiredOption = (id: string, value: string): Readonly<Record<string, unknown>> => ({
  id,
  currentValue: { kind: "select", value },
});
const correctOptions = [requiredOption("reasoning_effort", "low"), requiredOption("fast", "false")];

test("Cursor launch selection passes all guards without selecting a model on create", async () => {
  const observed = await runModelCase(
    { currentModel: "grok-4.7", configOptions: correctOptions },
    CURSOR_SELECTION,
  );
  assert.equal(observed.result.verdict.kind, "qualified");
  assert.equal(observed.createModelPresent, false);
  assert.equal(observed.createModel, undefined);
  assert.equal(observed.calls.includes("conversation_settings_set"), false);
  assert.equal(observed.calls.includes("conversation_settings_accept"), false);
  assert.equal(observed.calls.filter((name) => name === "conversation_prompt").length, 3);
});

for (const [label, configOptions, reason] of [
  [
    "wrong effort",
    [requiredOption("reasoning_effort", "high"), requiredOption("fast", "false")],
    "required option reasoning_effort",
  ],
  [
    "fast true",
    [requiredOption("reasoning_effort", "low"), requiredOption("fast", "true")],
    "required option fast",
  ],
  ["missing option", [requiredOption("reasoning_effort", "low")], "required option fast"],
  [
    "duplicated option",
    [...correctOptions, requiredOption("fast", "false")],
    "required option fast",
  ],
] as const) {
  for (const [phase, acceptedPrompts] of [
    ["initial", 0],
    ["resume", 1],
    ["load", 2],
  ] as const) {
    test(`required selection refuses ${label} before input at ${phase}`, async () => {
      const options: StandInOptions = {
        currentModel: "grok-4.7",
        configOptions: phase === "initial" ? configOptions : correctOptions,
        ...(phase === "resume" ? { configOptionsAfterResume: configOptions } : {}),
        ...(phase === "load" ? { configOptionsAfterLoad: configOptions } : {}),
      };
      const observed = await runModelCase(options, CURSOR_SELECTION);
      assert.equal(observed.result.verdict.kind, "failed");
      if (observed.result.verdict.kind !== "failed") throw new Error("expected failed guard");
      assert.match(observed.result.verdict.reason, new RegExp(reason));
      assert.equal(
        observed.calls.filter((name) => name === "conversation_prompt").length,
        acceptedPrompts,
      );
      if (phase !== "initial") {
        const reattached = observed.calls.lastIndexOf(
          phase === "load" ? "conversation_load" : "conversation_resume",
        );
        assert.ok(reattached >= 0);
        assert.equal(observed.calls.slice(reattached + 1).includes("conversation_prompt"), false);
      }
      assert.equal(observed.createModelPresent, false);
      assert.ok(observed.evidence.includes('"tool":"provider_session_inspect"'));
    });
  }
}

const exchangeSchema = z.looseObject({
  step: z.string(),
  tool: z.string().optional(),
  arguments: z.record(z.string(), z.unknown()).optional(),
  outcome: z.unknown().optional(),
});

test("restored Claude is reselected once after each settled restore, inspected afresh and then prompted", async () => {
  const observed = await runModelCase({
    modelAfterResume: "opus",
    modelAfterLoad: "opus",
    restoreSettingsGate: true,
  });
  assert.equal(observed.result.verdict.kind, "qualified");
  assert.equal(observed.settingsRequests.length, 2);
  assert.equal(observed.calls.includes("conversation_settings_accept"), false);
  const entries = observed.evidence
    .trim()
    .split("\n")
    .map((line) => exchangeSchema.parse(decode(line)));
  for (const tool of ["conversation_resume", "conversation_load"] as const) {
    const restore = entries.findIndex((entry) => entry.tool === tool);
    const set = entries.findIndex(
      (entry, index) => index > restore && entry.tool === "conversation_settings_set",
    );
    assert.ok(set > restore);
    const settlement = entries[set - 1];
    assert.equal(settlement?.step, `reattach-${tool}-settlement`);
    const retained = z
      .looseObject({
        value: z.looseObject({
          output: z.looseObject({
            kind: z.literal("available"),
            settlement: z.looseObject({ kind: z.enum(["resumed", "loaded"]) }),
          }),
        }),
      })
      .parse(settlement?.outcome);
    assert.equal(
      retained.value.output.settlement.kind,
      tool === "conversation_resume" ? "resumed" : "loaded",
    );
    const request = entries[set]?.arguments;
    assert.deepEqual(request, {
      target: observed.result.target,
      actor: { humanId: STAND_IN_OWNER },
      setting: "model",
      value: "haiku",
    });
    assert.equal(entries[set + 1]?.tool, "provider_session_inspect");
    assert.equal(entries[set + 1]?.step, `reattach-${tool}-model-inspect`);
    const prompt = entries.findIndex(
      (entry, index) => index > set && entry.tool === "conversation_prompt",
    );
    assert.ok(prompt > set);
    assert.equal(
      entries.slice(set + 1, prompt).some((entry) => entry.tool === "conversation_settings_set"),
      false,
    );
    assert.equal(entries[prompt - 1]?.tool, "provider_session_inspect");
  }
});

for (const phase of ["resume", "load"] as const) {
  const drift: StandInOptions =
    phase === "resume" ? { modelAfterResume: "opus" } : { modelAfterLoad: "opus" };
  const priorPrompts = phase === "resume" ? 1 : 2;
  test(`selectOnRestore false leaves the ${phase} default unchanged and sends no next prompt`, async () => {
    const observed = await runModelCase(drift, { ...CLAUDE_SELECTION, selectOnRestore: false });
    assert.equal(observed.result.verdict.kind, "failed");
    assert.match(
      observed.result.verdict.kind === "failed" ? observed.result.verdict.reason : "",
      /current model opus differs/,
    );
    assert.equal(observed.settingsRequests.length, 0);
    assert.equal(
      observed.calls.filter((name) => name === "conversation_prompt").length,
      priorPrompts,
    );
  });
  for (const [label, options, reason] of [
    ["provider rejected", { settingsSetOutcome: "providerRejected" }, /refused/],
    ["ambiguous", { settingsSetOutcome: "outcomeUnknown" }, /refused/],
    ["malformed success", { settingsSetOutcome: "malformed" }, /effectiveSettings/],
    ["foreign target", { settingsSetOutcome: "foreignTarget" }, /another Session/],
    ["inspect still opus", { settingsSetKeepsModel: true }, /current model opus differs/],
  ] as const) {
    test(`${phase} reselect stops with no prompt: ${label}`, async () => {
      const observed = await runModelCase(
        { ...drift, ...options, settingsSetPhase: phase },
        CLAUDE_SELECTION,
      );
      assert.equal(observed.result.verdict.kind, "failed");
      assert.match(
        observed.result.verdict.kind === "failed" ? observed.result.verdict.reason : "",
        reason,
      );
      const restore = observed.calls.lastIndexOf(
        phase === "resume" ? "conversation_resume" : "conversation_load",
      );
      assert.ok(restore >= 0);
      assert.equal(observed.calls.slice(restore + 1).includes("conversation_prompt"), false);
      assert.equal(
        observed.calls.slice(restore + 1).filter((name) => name === "conversation_settings_set")
          .length,
        1,
      );
      assert.equal(observed.calls.includes("conversation_settings_accept"), false);
    });
  }
}
