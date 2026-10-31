import assert from "node:assert/strict";
import { test } from "node:test";
import {
  journeyObservationPlanSchema,
  skillSubmissionSchema,
  type JourneyReadback,
  type ModelReadback,
  type NativeSkillsList,
  type NativeThread,
} from "../acp-journey-models.ts";
import {
  checkActiveJourney,
  checkIdleJourney,
  checkTurnInProgress,
  nativeThreadAlias,
  uuidV5,
  checkCompactRefused,
  checkCompacted,
  checkEffectiveModel,
  checkModelCatalogue,
  checkSkillEffect,
  checkSkillsListed,
  skillReferencePath,
} from "../acp-journey-oracle.ts";

// Expected values are literals from Program Design's reference format and Specification C1-C3,
// never computed by the code under test.
const SESSION = "019a0000-0000-7000-8000-000000000001";
const CWD = "/work/repo";
const TARGET = { endpointId: "claude-local", sessionId: SESSION, cwd: CWD };
const FIXTURE_PATH = `/__codex_router_skill__/claude-local/MDE5YTAwMDAtMDAwMC03MDAwLTgwMDAtMDAwMDAwMDAwMDAx/Zml4dHVyZS1za2lsbA`;
const MARKER = "SKILLFX_7Q2K";

function skillRow(name: string, path: string, scope = "repo", enabled = true) {
  return { name, description: "fixture", path, scope, enabled, pluginId: null };
}
function list(entries: NativeSkillsList["data"]): NativeSkillsList {
  return { data: entries };
}
const BOUND = list([
  {
    cwd: CWD,
    skills: [
      skillRow("fixture-skill", FIXTURE_PATH),
      skillRow(
        "owner-skill",
        `/__codex_router_skill__/claude-local/MDE5YTAwMDAtMDAwMC03MDAwLTgwMDAtMDAwMDAwMDAwMDAx/b3duZXItc2tpbGw`,
        "user",
      ),
    ],
    errors: [],
  },
]);
function thread(turns: NativeThread["turns"]): NativeThread {
  return { id: SESSION, cwd: CWD, status: { type: "idle" }, turns };
}
const userText = (id: string, text: string) => ({
  id,
  type: "userMessage",
  content: [{ type: "text", text, text_elements: [] }],
});
const agent = (id: string, text: string) => ({ id, type: "agentMessage", text });
const ORDINARY = {
  id: "turn-1",
  status: "completed",
  items: [userText("u1", "hello"), agent("a1", "HI")],
};
const SKILL_TURN = {
  id: "turn-2",
  status: "completed",
  items: [userText("u2", "$fixture-skill alpha beta"), agent("a2", `${MARKER} alpha beta`)],
};
function readback(overrides: Partial<JourneyReadback> = {}): JourneyReadback {
  const history = thread([ORDINARY, SKILL_TURN]);
  return {
    unboundSkills: list([]),
    boundSkills: BOUND,
    threadRead: history,
    threadResume: history,
    resumedModel: "claude-haiku-5-5",
    models: null,
    providerAgentTexts: ["HI", `${MARKER} alpha beta`],
    providerToolItems: [],
    ...overrides,
  };
}
const SUBMISSION = skillSubmissionSchema.parse({
  original_text: "$fixture-skill alpha beta",
  selected: ["fixture-skill"],
  image_count: 0,
  effect_marker: MARKER,
});
const EXPECTED = [{ invocation_name: "fixture-skill", scope: "repo" as const }];

test("selection reference is the literal base64url-without-padding form", (): void => {
  assert.equal(skillReferencePath("claude-local", SESSION, "fixture-skill"), FIXTURE_PATH);
  assert.equal(
    skillReferencePath("cursor-local", SESSION, "café-skill"),
    `/__codex_router_skill__/cursor-local/MDE5YTAwMDAtMDAwMC03MDAwLTgwMDAtMDAwMDAwMDAwMDAx/Y2Fmw6ktc2tpbGw`,
  );
  // A legal session ID with slashes and dot segments stays one opaque component.
  assert.equal(
    skillReferencePath("claude-local", "sessions/../a b", "fixture-skill"),
    "/__codex_router_skill__/claude-local/c2Vzc2lvbnMvLi4vYSBi/Zml4dHVyZS1za2lsbA",
  );
});

test("skills-listed accepts the bound fixture skill beside other provider skills", (): void => {
  for (const unbound of [list([]), list([{ cwd: CWD, skills: [], errors: [] }])])
    assert.deepEqual(
      checkSkillsListed(readback({ unboundSkills: unbound }), TARGET, EXPECTED, ["compact"]),
      { kind: "skills-listed", observed: { listed: ["fixture-skill", "owner-skill"], errors: 0 } },
    );
});

const listedFailures: ReadonlyArray<[string, Partial<JourneyReadback>, string[], RegExp]> = [
  [
    "empty native face",
    { boundSkills: list([{ cwd: CWD, skills: [], errors: [] }]) },
    [],
    /expected skill missing: fixture-skill/,
  ],
  ["no entry for the cwd", { boundSkills: list([]) }, [], /exactly the requested cwd/],
  [
    "entry for another cwd",
    {
      boundSkills: list([
        { cwd: "/other", skills: [skillRow("fixture-skill", FIXTURE_PATH)], errors: [] },
      ]),
    },
    [],
    /exactly the requested cwd/,
  ],
  [
    "unbound connection borrows the TUI binding",
    { unboundSkills: BOUND },
    [],
    /not connection-local/,
  ],
  [
    "command advertised by the provider listed as a skill",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [skillRow("fixture-skill", FIXTURE_PATH), skillRow("compact", FIXTURE_PATH)],
          errors: [],
        },
      ]),
    },
    ["compact"],
    /advertised command listed as a skill: compact/,
  ],
  [
    "selection path addresses another session",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [
            skillRow(
              "fixture-skill",
              `/__codex_router_skill__/claude-local/MDE5YTAwMDAtMDAwMC03MDAwLTgwMDAtMDAwMDAwMDAwMDAy/Zml4dHVyZS1za2lsbA`,
            ),
          ],
          errors: [],
        },
      ]),
    },
    [],
    /does not address this session/,
  ],
  [
    "an unencoded session component",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [
            skillRow(
              "fixture-skill",
              `/__codex_router_skill__/claude-local/${SESSION}/Zml4dHVyZS1za2lsbA`,
            ),
          ],
          errors: [],
        },
      ]),
    },
    [],
    /does not address this session/,
  ],
  [
    "a padded name encoding",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [
            skillRow(
              "fixture-skill",
              `/__codex_router_skill__/claude-local/MDE5YTAwMDAtMDAwMC03MDAwLTgwMDAtMDAwMDAwMDAwMDAx/Zml4dHVyZS1za2lsbA==`,
            ),
          ],
          errors: [],
        },
      ]),
    },
    [],
    /does not address this session/,
  ],
  [
    "physical skill file path instead of the server reference",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [skillRow("fixture-skill", "/work/repo/.claude/skills/fixture-skill/SKILL.md")],
          errors: [],
        },
      ]),
    },
    [],
    /does not address this session/,
  ],
  [
    "wrong scope",
    {
      boundSkills: list([
        { cwd: CWD, skills: [skillRow("fixture-skill", FIXTURE_PATH, "system")], errors: [] },
      ]),
    },
    [],
    /skill scope differs/,
  ],
  [
    "duplicate row",
    {
      boundSkills: list([
        {
          cwd: CWD,
          skills: [
            skillRow("fixture-skill", FIXTURE_PATH),
            skillRow("fixture-skill", FIXTURE_PATH),
          ],
          errors: [],
        },
      ]),
    },
    [],
    /skill listed twice/,
  ],
  [
    "disabled row",
    {
      boundSkills: list([
        { cwd: CWD, skills: [skillRow("fixture-skill", FIXTURE_PATH, "repo", false)], errors: [] },
      ]),
    },
    [],
    /skill listed disabled/,
  ],
];
for (const [name, overrides, excluded, error] of listedFailures)
  test(`skills-listed rejects ${name}`, (): void => {
    assert.throws(() => checkSkillsListed(readback(overrides), TARGET, EXPECTED, excluded), error);
  });

test("skill-effect accepts one completed effect turn holding the exact original input", (): void => {
  assert.deepEqual(checkSkillEffect(readback(), SUBMISSION), {
    kind: "skill-effect",
    observed: { turn_id: "turn-2" },
  });
});

test("skill-effect accepts image input without a synthesized user item", (): void => {
  const imageTurn = { id: "turn-2", status: "completed", items: [agent("a2", `${MARKER} alpha`)] };
  const history = thread([ORDINARY, imageTurn]);
  assert.deepEqual(
    checkSkillEffect(
      readback({ threadRead: history, threadResume: history }),
      skillSubmissionSchema.parse({ ...SUBMISSION, image_count: 1 }),
    ),
    { kind: "skill-effect", observed: { turn_id: "turn-2" } },
  );
});

function withSkillTurn(items: NativeThread["turns"][number]["items"], status = "completed") {
  const history = thread([ORDINARY, { id: "turn-2", status, items }]);
  return { threadRead: history, threadResume: history };
}
const effectFailures: ReadonlyArray<[string, Partial<JourneyReadback>, RegExp]> = [
  [
    "an echo or explanation without the skill's own marker",
    withSkillTurn([
      userText("u2", "$fixture-skill alpha beta"),
      agent("a2", "I will run $fixture-skill alpha beta"),
    ]),
    /skill effect absent/,
  ],
  [
    "the effect twice",
    (() => {
      const history = thread([SKILL_TURN, { ...SKILL_TURN, id: "turn-3" }]);
      return { threadRead: history, threadResume: history };
    })(),
    /skill effect repeated/,
  ],
  [
    "Claude's prepared invocation stored as the user item",
    withSkillTurn([userText("u2", "/fixture-skill alpha beta"), agent("a2", MARKER)]),
    /not the exact original \$name input/,
  ],
  [
    "the prepared two-block form stored as the user item",
    withSkillTurn([
      {
        id: "u2",
        type: "userMessage",
        content: [
          { type: "text", text: "please " },
          { type: "text", text: "/fixture-skill alpha beta" },
        ],
      },
      agent("a2", MARKER),
    ]),
    /not the exact original \$name input/,
  ],
  [
    "a second synthesized user item",
    withSkillTurn([
      userText("u2", "$fixture-skill alpha beta"),
      userText("u3", "/fixture-skill alpha beta"),
      agent("a2", MARKER),
    ]),
    /not the exact original \$name input/,
  ],
  [
    "an interrupted skill turn",
    withSkillTurn(
      [userText("u2", "$fixture-skill alpha beta"), agent("a2", MARKER)],
      "interrupted",
    ),
    /did not complete/,
  ],
  [
    "provider events without the effect",
    { providerAgentTexts: ["HI"] },
    /provider events lack the skill effect/,
  ],
  [
    "read and resume disagreeing on the turn",
    { threadResume: thread([{ ...SKILL_TURN, id: "turn-9" }]) },
    /read and resume disagree/,
  ],
];
for (const [name, overrides, error] of effectFailures)
  test(`skill-effect rejects ${name}`, (): void => {
    assert.throws(() => checkSkillEffect(readback(overrides), SUBMISSION), error);
  });

test("skill-effect rejects a synthesized user item for image input", (): void => {
  assert.throws(
    () =>
      checkSkillEffect(readback(), skillSubmissionSchema.parse({ ...SUBMISSION, image_count: 1 })),
    /image input synthesized a user history item/,
  );
});

test("a submission whose marker or selection an echo could satisfy is refused before observing", (): void => {
  for (const [patch, error] of [
    [{ effect_marker: "fixture-skill" }, /echo could satisfy it/],
    [{ original_text: `$fixture-skill ${MARKER}` }, /echo could satisfy it/],
    [{ original_text: "fixture-skill alpha" }, /appear as \$name/],
  ] as const)
    assert.throws(() => skillSubmissionSchema.parse({ ...SUBMISSION, ...patch }), error);
});

const COMPACT_TURN = {
  id: "turn-3",
  status: "completed",
  items: [{ id: "c1", type: "contextCompaction" }],
};
const COMPACTED = (() => {
  const history = thread([ORDINARY, COMPACT_TURN]);
  return { threadRead: history, threadResume: history };
})();
const MARKER_ITEM = { itemId: "tool-1", toolKind: "contextCompaction", status: "completed" };

test("compacted needs the native item, a completed turn and the provider's typed marker", (): void => {
  assert.deepEqual(checkCompacted(readback({ ...COMPACTED, providerToolItems: [MARKER_ITEM] })), {
    kind: "compacted",
    observed: { turn_id: "turn-3" },
  });
});

const compactFailures: ReadonlyArray<[string, Partial<JourneyReadback>, RegExp]> = [
  [
    "an acknowledgement or reply standing in for compaction",
    {
      ...withSkillTurn([agent("a3", "Compacted the conversation.")]),
      providerToolItems: [MARKER_ITEM],
    },
    /lacks exactly one contextCompaction item/,
  ],
  [
    "a tool titled Compact without the typed marker",
    {
      ...COMPACTED,
      providerToolItems: [{ itemId: "t", toolKind: "Compact", status: "completed" }],
    },
    /lack a completed typed compaction marker/,
  ],
  [
    "a failed provider marker",
    { ...COMPACTED, providerToolItems: [{ ...MARKER_ITEM, status: "failed" }] },
    /lack a completed typed compaction marker/,
  ],
  [
    "a failed compaction turn",
    (() => {
      const history = thread([ORDINARY, { ...COMPACT_TURN, status: "failed" }]);
      return { threadRead: history, threadResume: history, providerToolItems: [MARKER_ITEM] };
    })(),
    /compaction turn did not complete/,
  ],
  [
    "the prepared /compact prompt in user history",
    (() => {
      const history = thread([
        ORDINARY,
        {
          ...COMPACT_TURN,
          items: [userText("u3", "/compact"), { id: "c1", type: "contextCompaction" }],
        },
      ]);
      return { threadRead: history, threadResume: history, providerToolItems: [MARKER_ITEM] };
    })(),
    /prepared \/compact prompt leaked/,
  ],
];
for (const [name, overrides, error] of compactFailures)
  test(`compacted rejects ${name}`, (): void => {
    assert.throws(() => checkCompacted(readback(overrides)), error);
  });

test("compact-refused accepts unchanged history and no provider compaction", (): void => {
  const before = thread([ORDINARY]);
  assert.deepEqual(
    checkCompactRefused(readback({ threadRead: before, threadResume: before }), before),
    { kind: "compact-refused", observed: { turns: 1 } },
  );
});

test("compact-refused rejects a sent prompt turn or a provider compaction", (): void => {
  const before = thread([ORDINARY]);
  const sent = thread([
    ORDINARY,
    { id: "turn-2", status: "completed", items: [agent("a2", "/compress done")] },
  ]);
  assert.throws(
    () => checkCompactRefused(readback({ threadRead: sent, threadResume: sent }), before),
    /refused compact changed Codex history/,
  );
  assert.throws(
    () =>
      checkCompactRefused(
        readback({ threadRead: before, threadResume: before, providerToolItems: [MARKER_ITEM] }),
        before,
      ),
    /refused compact reached the provider/,
  );
});

test("effective-model compares the resumed model with the TUI selection", (): void => {
  assert.deepEqual(checkEffectiveModel(readback(), "claude-haiku-5-5"), {
    kind: "effective-model",
    observed: { model: "claude-haiku-5-5" },
  });
  assert.throws(
    () => checkEffectiveModel(readback({ resumedModel: "provider-default" }), "claude-haiku-5-5"),
    /effective model differs/,
  );
});

test("the journey plan requires an absolute bound cwd and at least one check", (): void => {
  const base = {
    repository: "/repo",
    source_head: "head",
    build_manifest: "/repo/manifest.json",
    host_pid: 1,
    host_binary: "/bin/host",
    host_sha256: "h",
    router_binary: "/bin/router",
    router_sha256: "r",
    codex_cli: "/bin/codex",
    codex_sha256: "c",
    collaboration_cli: "/bin/agent-collaboration",
    service_directory: "/service",
    endpoint_id: "claude-local",
    session_id: SESSION,
    native_thread_id: SESSION,
    evidence: "/evidence",
    cwd: CWD,
    mode: "idle-bind",
    checks: [{ kind: "effective-model", expected_model: "claude-haiku-5-5" }],
  };
  assert.equal(journeyObservationPlanSchema.parse(base).cwd, CWD);
  // Raw provider Session IDs are opaque provider data, not necessarily UUIDs.
  assert.equal(
    journeyObservationPlanSchema.parse({ ...base, session_id: "chat/../a b" }).session_id,
    "chat/../a b",
  );
  assert.throws(
    () => journeyObservationPlanSchema.parse({ ...base, mode: "active-read-only" }),
    /active-read-only allows only turn-in-progress/,
  );
  assert.throws(
    () => journeyObservationPlanSchema.parse({ ...base, checks: [{ kind: "turn-in-progress" }] }),
    /active-read-only allows only turn-in-progress/,
  );
  assert.equal(
    journeyObservationPlanSchema.parse({
      ...base,
      mode: "active-read-only",
      checks: [{ kind: "turn-in-progress" }],
    }).mode,
    "active-read-only",
  );
  assert.throws(
    () => journeyObservationPlanSchema.parse({ ...base, cwd: "repo" }),
    /absolute path required/,
  );
  assert.throws(() => journeyObservationPlanSchema.parse({ ...base, checks: [] }));
  assert.throws(() => journeyObservationPlanSchema.parse({ ...base, terminal_capture: "/x" }));
});

test("UUIDv5 reproduces the published RFC 4122 name-based vector", (): void => {
  // Python's uuid documentation: uuid5(NAMESPACE_DNS, "python.org").
  assert.equal(
    uuidV5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "python.org"),
    "886313e1-3b8a-5372-9b90-0c9aee199e5d",
  );
});

test("the native thread alias keeps UUID text the Router parses and hashes everything else", (): void => {
  const service = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
  // Canonical form of the four text forms uuid 1.26.0 `Uuid::parse_str` accepts.
  for (const accepted of [
    "019A0000-0000-7000-8000-0000000000AB",
    "019a00000000700080000000000000aB",
    "{019a0000-0000-7000-8000-0000000000ab}",
    "urn:uuid:019a0000-0000-7000-8000-0000000000ab",
  ])
    assert.equal(
      nativeThreadAlias(service, "claude-local", accepted),
      "019a0000-0000-7000-8000-0000000000ab",
      accepted,
    );
  // Opaque IDs, including a slash/dot/space ID and a URN prefix the crate does not match, are
  // UUIDv5(URL namespace, "codex-router:session:<service>:<endpoint>:<raw>"). The literal was
  // derived separately with xxd and shasum and hand-set version/variant bits.
  assert.equal(
    nativeThreadAlias(service, "cursor-local", "chat/../a b"),
    "5756efda-4d21-59ce-aef2-2b68a02a6598",
  );
  assert.notEqual(
    nativeThreadAlias(service, "claude-local", "URN:UUID:019a0000-0000-7000-8000-0000000000ab"),
    "019a0000-0000-7000-8000-0000000000ab",
  );
  assert.notEqual(
    nativeThreadAlias(service, "claude-local", "chat/../a b"),
    nativeThreadAlias(service, "cursor-local", "chat/../a b"),
  );
});

test("turn-in-progress reads only an active turn and the dispatchers keep the modes apart", (): void => {
  const active = thread([ORDINARY, { id: "turn-2", status: "inProgress", items: [] }]);
  assert.deepEqual(checkTurnInProgress({ threadRead: active }), {
    kind: "turn-in-progress",
    observed: { turn_id: "turn-2" },
  });
  assert.throws(
    () => checkTurnInProgress({ threadRead: thread([ORDINARY]) }),
    /no in-progress turn/,
  );
  assert.throws(
    () => checkActiveJourney({ threadRead: active }, [{ kind: "compacted" }]),
    /compacted needs an idle-bind observation/,
  );
  assert.throws(
    () => checkIdleJourney(readback(), TARGET, [{ kind: "turn-in-progress" }], () => thread([])),
    /turn-in-progress needs an active-read-only observation/,
  );
});

// The provider Session's own catalogue (session inspect) and the bridge's native lists, as literals.
const providerCatalog = (model: string, effort: { current: string; choices: string[] } | null) => ({
  configOptions: [
    {
      id: "model",
      category: "model" as const,
      currentValue: { kind: "select" as const, value: model },
      choices: ["sonnet", "haiku"].map((value) => ({ value, label: value })),
    },
    ...(effort === null
      ? []
      : [
          {
            id: "effort",
            category: "effort" as const,
            currentValue: { kind: "select" as const, value: effort.current },
            choices: effort.choices.map((value) => ({ value, label: value })),
          },
        ]),
  ],
});
const nativeRow = (model: string, efforts: string[], defaultEffort: string) => ({
  model,
  displayName: model,
  supportedReasoningEfforts: efforts.map((reasoningEffort) => ({
    reasoningEffort,
    description: "",
  })),
  defaultReasoningEffort: defaultEffort,
});
const DELEGATION = nativeRow("provider-default", ["provider"], "provider");
function modelReads(overrides: Partial<ModelReadback> = {}): ModelReadback {
  return {
    unboundModels: { data: [DELEGATION] },
    boundModels: {
      data: [
        nativeRow("sonnet", ["default", "high"], "default"),
        nativeRow("haiku", ["provider"], "provider"),
      ],
    },
    providerCatalog: providerCatalog("sonnet", { current: "high", choices: ["default", "high"] }),
    resumedEffort: "high",
    ...overrides,
  };
}
const QUALIFIED = {
  kind: "model-catalogue" as const,
  delegation_models: ["provider-default"],
  selected_model: "sonnet",
  effort: { kind: "qualified" as const, expected_current: "high" },
};

// Each case breaks one rule; the check must refuse it with that rule's reason.
const MODEL_CATALOGUE_FAILURES: readonly [string, Partial<JourneyReadback>, RegExp][] = [
  ["no model reads", { resumedModel: "sonnet", models: null }, /needs the observer's model reads/],
  [
    "a provider model hidden",
    {
      resumedModel: "sonnet",
      models: modelReads({
        boundModels: { data: [nativeRow("sonnet", ["default", "high"], "default")] },
      }),
    },
    /hides provider models: haiku/,
  ],
  [
    "an invented model",
    {
      resumedModel: "sonnet",
      models: modelReads({
        boundModels: {
          data: [...modelReads().boundModels.data, nativeRow("opus", ["high"], "high")],
        },
      }),
    },
    /adds models: opus/,
  ],
  [
    "bootstrap with a real model",
    {
      resumedModel: "sonnet",
      models: modelReads({
        unboundModels: { data: [DELEGATION, nativeRow("sonnet", ["high"], "high")] },
      }),
    },
    /bootstrap catalogue is not the delegation alone/,
  ],
  [
    "provider current model elsewhere",
    {
      resumedModel: "sonnet",
      models: modelReads({
        providerCatalog: providerCatalog("haiku", {
          current: "high",
          choices: ["default", "high"],
        }),
      }),
    },
    /provider current model differs/,
  ],
  [
    "native current model stale",
    { resumedModel: "haiku", models: modelReads() },
    /native current model differs/,
  ],
  // A broken extension that leaves every model in the fallback cannot pass the qualified case.
  [
    "every model in fallback",
    {
      resumedModel: "sonnet",
      models: modelReads({
        boundModels: {
          data: [
            nativeRow("sonnet", ["provider"], "provider"),
            nativeRow("haiku", ["provider"], "provider"),
          ],
        },
      }),
    },
    /native effort choices differ from the provider's options/,
  ],
  [
    "a guessed medium default",
    {
      resumedModel: "sonnet",
      models: modelReads({
        boundModels: {
          data: [
            nativeRow("sonnet", ["default", "high"], "medium"),
            nativeRow("haiku", ["provider"], "provider"),
          ],
        },
      }),
    },
    /native default effort is not an offered provider value/,
  ],
  [
    "the wrong current effort",
    {
      resumedModel: "sonnet",
      models: modelReads({
        providerCatalog: providerCatalog("sonnet", {
          current: "default",
          choices: ["default", "high"],
        }),
      }),
    },
    /provider current effort differs/,
  ],
  [
    "native effort not the provider's",
    { resumedModel: "sonnet", models: modelReads({ resumedEffort: "default" }) },
    /native current effort differs/,
  ],
];

test("the model catalogue is judged against the provider Session, never the native list", (): void => {
  assert.deepEqual(
    checkModelCatalogue(readback({ resumedModel: "sonnet", models: modelReads() }), QUALIFIED)
      .observed,
    {
      selected_model: "sonnet",
      provider_models: 2,
      native_models: 2,
      bootstrap_models: ["provider-default"],
      effort: "qualified",
      native_efforts: ["default", "high"],
      current_effort: "high",
    },
  );
  for (const [name, overrides, failure] of MODEL_CATALOGUE_FAILURES)
    assert.throws(() => checkModelCatalogue(readback(overrides), QUALIFIED), failure, name);
});

test("provider settings is an explicit action and never an effective effort", (): void => {
  const fallback = {
    ...QUALIFIED,
    selected_model: "haiku",
    effort: { kind: "provider-settings" as const, action: "provider" },
  };
  // The provider still reports its own current effort; the action changes nothing about it.
  const current = modelReads({
    providerCatalog: providerCatalog("haiku", { current: "default", choices: ["default"] }),
    resumedEffort: "default",
  });
  assert.equal(
    checkModelCatalogue(readback({ resumedModel: "haiku", models: current }), fallback).observed
      .effort,
    "provider-settings",
  );
  // No provider effort option at all: the effective effort is null, not the action.
  const none = modelReads({ providerCatalog: providerCatalog("haiku", null), resumedEffort: null });
  assert.equal(
    checkModelCatalogue(readback({ resumedModel: "haiku", models: none }), fallback).observed
      .current_effort,
    null,
  );
  assert.throws(
    () =>
      checkModelCatalogue(
        readback({ resumedModel: "haiku", models: { ...none, resumedEffort: "provider" } }),
        fallback,
      ),
    /provider-settings action is reported as an effective effort/,
  );
  // Collision: the provider itself reports the action string as its current effort. Matching
  // upstream data passes; the same native value against a different provider value does not.
  const collision = modelReads({
    providerCatalog: providerCatalog("haiku", {
      current: "provider",
      choices: ["provider", "high"],
    }),
    resumedEffort: "provider",
  });
  assert.equal(
    checkModelCatalogue(readback({ resumedModel: "haiku", models: collision }), fallback).observed
      .current_effort,
    "provider",
  );
  assert.throws(
    () =>
      checkModelCatalogue(
        readback({ resumedModel: "haiku", models: { ...current, resumedEffort: "provider" } }),
        fallback,
      ),
    /provider-settings action is reported as an effective effort/,
  );
  // A fallback row must carry exactly the declared action.
  const missingAction = modelReads({
    boundModels: {
      data: [
        nativeRow("sonnet", ["default", "high"], "default"),
        nativeRow("haiku", ["medium"], "medium"),
      ],
    },
    providerCatalog: providerCatalog("haiku", null),
    resumedEffort: null,
  });
  assert.throws(
    () => checkModelCatalogue(readback({ resumedModel: "haiku", models: missingAction }), fallback),
    /lacks the explicit provider-settings action/,
  );
});
