import assert from "node:assert/strict";
import { test } from "node:test";
import {
  journeyCheckSchema,
  journeyObservationPlanSchema,
  type JourneyReadback,
  type NativeThread,
} from "./acp-journey-models.ts";
import { checkIdleJourney, checkOrdinaryReply } from "./acp-journey-oracle.ts";

// Literal native Codex history shapes (v2 thread items), never computed by the code under test.
const SESSION = "019a0000-0000-7000-8000-000000000001";
const CWD = "/work/repo";
const PROMPT = "count slowly from one to forty, then say the closing word";
const COMPLETION = "REFRESH_DONE_4K7Q";
const CHECK = journeyCheckSchema.parse({
  kind: "ordinary-reply",
  prompt: PROMPT,
  completion: COMPLETION,
});
if (CHECK.kind !== "ordinary-reply") throw new Error("ordinary-reply check expected");

type Turn = NativeThread["turns"][number];
type Item = Turn["items"][number];
const userText = (id: string, text: string): Item => ({
  id,
  type: "userMessage",
  content: [{ type: "text", text, text_elements: [] }],
});
const agent = (id: string, text: string): Item => ({ id, type: "agentMessage", text });
const EARLIER: Turn = {
  id: "turn-1",
  status: "completed",
  items: [userText("u1", "hello"), agent("a1", "HI")],
};
const REPLY: Turn = {
  id: "turn-2",
  status: "completed",
  items: [userText("u2", PROMPT), agent("a2", `one, two, three\n${COMPLETION}`)],
};
function thread(turns: readonly Turn[]): NativeThread {
  return { id: SESSION, cwd: CWD, status: { type: "idle" }, turns: [...turns] };
}
function readback(
  turns: readonly Turn[],
  overrides: Partial<JourneyReadback> = {},
): JourneyReadback {
  return {
    unboundSkills: { data: [] },
    boundSkills: { data: [] },
    threadRead: thread(turns),
    threadResume: thread(turns),
    resumedModel: "sonnet",
    models: null,
    information: null,
    claudeRoute: null,
    providerAgentTexts: ["HI", `one, two, three\n${COMPLETION}`],
    providerToolItems: [],
    ...overrides,
  };
}
const withReply = (reply: Partial<Turn>): Turn[] => [EARLIER, { ...REPLY, ...reply }];

test("ordinary-reply accepts the latest completed turn with the exact prompt and its reply", (): void => {
  assert.deepEqual(checkOrdinaryReply(readback([EARLIER, REPLY]), CHECK), {
    kind: "ordinary-reply",
    observed: { turn_id: "turn-2" },
  });
  assert.deepEqual(
    checkIdleJourney(
      readback([EARLIER, REPLY]),
      { endpointId: "claude-local", sessionId: SESSION, cwd: CWD },
      [CHECK],
      () => thread([]),
    ),
    [{ kind: "ordinary-reply", observed: { turn_id: "turn-2" } }],
  );
});

test("the completion line may sit anywhere in a multi-line reply, apart from surrounding space", (): void => {
  const multiline = `Counting done.\r\n  ${COMPLETION}  \n\nBye`;
  assert.deepEqual(
    checkOrdinaryReply(
      readback(withReply({ items: [userText("u2", PROMPT), agent("a2", multiline)] }), {
        providerAgentTexts: [multiline],
      }),
      CHECK,
    ),
    { kind: "ordinary-reply", observed: { turn_id: "turn-2" } },
  );
});

// The marker inside other text is not the completion line: a prefix, a suffix or failure prose.
const EMBEDDED = [
  `x ${COMPLETION}`,
  `${COMPLETION} extra`,
  `${COMPLETION}.`,
  `Error: the tool failed before it could print ${COMPLETION} for you.`,
];

const replyFailures: ReadonlyArray<[string, JourneyReadback, RegExp]> = [
  ...EMBEDDED.map((text): [string, JourneyReadback, RegExp] => [
    `native reply embeds the marker: ${text}`,
    readback(withReply({ items: [userText("u2", PROMPT), agent("a2", text)] }), {
      providerAgentTexts: [text],
    }),
    /lacks the assistant completion line/,
  ]),
  ...EMBEDDED.map((text): [string, JourneyReadback, RegExp] => [
    `public events embed the marker: ${text}`,
    readback([EARLIER, REPLY], { providerAgentTexts: ["HI", text] }),
    /provider events lack the completion line/,
  ]),
  ["no turn at all", readback([]), /no reply turn/],
  // The completion only in an earlier turn: a prior reply in this history.
  [
    "completion only in a prior turn",
    readback([{ ...EARLIER, items: [userText("u1", "hello"), agent("a1", COMPLETION)] }, REPLY]),
    /earlier turn already holds/,
  ],
  [
    "stale reply and no new turn",
    readback([{ ...REPLY, id: "turn-1" }, EARLIER]),
    /earlier turn already holds/,
  ],
  // The completion only in the user's own item: a composer echo, not a reply.
  [
    "echo in the user item",
    readback(withReply({ items: [userText("u2", `${PROMPT} ${COMPLETION}`)] })),
    /lacks the assistant completion/,
  ],
  ["no assistant reply", readback(withReply({ items: [userText("u2", PROMPT)] })), /lacks/],
  [
    "a partial reply",
    readback(withReply({ items: [userText("u2", PROMPT), agent("a2", "REFRESH_DONE")] })),
    /lacks the assistant completion/,
  ],
  ["interrupted turn", readback(withReply({ status: "interrupted" })), /did not complete/],
  ["still running", readback(withReply({ status: "inProgress" })), /did not complete/],
  [
    "another prompt",
    readback(withReply({ items: [userText("u2", "count slowly"), agent("a2", COMPLETION)] })),
    /exact original prompt/,
  ],
  [
    "the prompt with an image",
    readback(
      withReply({
        items: [
          {
            id: "u2",
            type: "userMessage",
            content: [
              { type: "text", text: PROMPT },
              { type: "localImage", path: "/task/probe.png" },
            ],
          },
          agent("a2", COMPLETION),
        ],
      }),
    ),
    /exact original prompt/,
  ],
  [
    "a second submitted user item",
    readback(
      withReply({
        items: [userText("u2", PROMPT), userText("u3", "/skills"), agent("a2", COMPLETION)],
      }),
    ),
    /exact original prompt/,
  ],
  [
    "read and resume disagree",
    readback([EARLIER, REPLY], { threadResume: thread([EARLIER, { ...REPLY, id: "turn-9" }]) }),
    /read and resume disagree/,
  ],
  [
    "resume misses the reply",
    readback([EARLIER, REPLY], { threadResume: thread([EARLIER]) }),
    /lacks the assistant completion/,
  ],
  [
    "public events lack the reply",
    readback([EARLIER, REPLY], { providerAgentTexts: ["HI"] }),
    /provider events lack the completion/,
  ],
];

test("ordinary-reply rejects echo, stale or partial replies, unfinished turns and mismatched views", (): void => {
  for (const [name, view, expected] of replyFailures)
    assert.throws(() => checkOrdinaryReply(view, CHECK), expected, name);
});

test("an echo-satisfiable completion is refused, and the check needs an idle-bind readback", (): void => {
  for (const [completion, expected] of [
    ["count slowly", /an echo could satisfy it/],
    ["DONE", /too_small|>=8/],
    [`${COMPLETION}\nmore`, /the completion is one line/],
  ] as const)
    assert.throws(
      () => journeyCheckSchema.parse({ kind: "ordinary-reply", prompt: PROMPT, completion }),
      expected,
      completion,
    );
  const plan = {
    repository: "/work/repo",
    source_head: "a".repeat(40),
    build_manifest: "/task/build.json",
    host_pid: 42,
    host_binary: "/task/host",
    host_sha256: "b".repeat(64),
    router_binary: "/task/router",
    router_sha256: "c".repeat(64),
    codex_cli: "/task/codex",
    codex_sha256: "d".repeat(64),
    collaboration_cli: "/task/agent-collaboration",
    service_directory: "/task/service",
    endpoint_id: "claude-local",
    session_id: SESSION,
    native_thread_id: SESSION,
    cwd: CWD,
    evidence: "/task/evidence",
  };
  assert.doesNotThrow(() =>
    journeyObservationPlanSchema.parse({ ...plan, mode: "idle-bind", checks: [CHECK] }),
  );
  assert.throws(
    () =>
      journeyObservationPlanSchema.parse({ ...plan, mode: "active-read-only", checks: [CHECK] }),
    /active-read-only allows only turn-in-progress/,
  );
});
