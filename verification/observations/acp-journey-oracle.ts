import type {
  ExpectedSkill,
  JourneyCheck,
  JourneyCheckResult,
  JourneyReadback,
  NativeSkillsList,
  NativeThread,
  NativeThreadItem,
  SkillSubmission,
} from "./acp-journey-models.ts";

/**
 * The server-owned selection reference Program Design fixes for provider skills:
 * `/__codex_router_skill__/<endpoint>/<session>/<invocation name as UTF-8 base64url, no padding>`.
 */
export function skillReferencePath(
  endpointId: string,
  sessionId: string,
  invocationName: string,
): string {
  const encoded = Buffer.from(invocationName, "utf8").toString("base64url");
  return `/__codex_router_skill__/${endpointId}/${sessionId}/${encoded}`;
}

function onlyEntryFor(list: NativeSkillsList, cwd: string): NativeSkillsList["data"][number] {
  const [entry, ...extra] = list.data;
  if (entry === undefined || extra.length > 0 || entry.cwd !== cwd)
    throw new Error("skills/list must answer exactly the requested cwd");
  return entry;
}

export function checkSkillsListed(
  readback: JourneyReadback,
  target: { readonly endpointId: string; readonly sessionId: string; readonly cwd: string },
  expectedSkills: readonly ExpectedSkill[],
  excludedNames: readonly string[],
): JourneyCheckResult {
  // C1 requires an immediate empty view before this connection binds the cwd; it does not fix
  // whether that view is no entry or an empty entry, so both are accepted.
  if (
    readback.unboundSkills.data.some(
      (entry) => entry.cwd !== target.cwd || entry.skills.length > 0 || entry.errors.length > 0,
    ) ||
    readback.unboundSkills.data.length > 1
  )
    throw new Error("unbound connection listed skills; cwd binding is not connection-local");
  const bound = onlyEntryFor(readback.boundSkills, target.cwd);
  for (const expected of expectedSkills) {
    const rows = bound.skills.filter((row) => row.name === expected.invocation_name);
    if (rows.length === 0) throw new Error(`expected skill missing: ${expected.invocation_name}`);
    if (rows.length > 1) throw new Error(`skill listed twice: ${expected.invocation_name}`);
    const [row] = rows;
    if (row === undefined) throw new Error("unreachable: counted skill row missing");
    if (
      row.path !== skillReferencePath(target.endpointId, target.sessionId, expected.invocation_name)
    )
      throw new Error(`skill selection path does not address this session: ${row.name}`);
    if (row.scope !== expected.scope) throw new Error(`skill scope differs: ${row.name}`);
    if (!row.enabled) throw new Error(`skill listed disabled: ${row.name}`);
  }
  for (const name of excludedNames)
    if (bound.skills.some((row) => row.name === name))
      throw new Error(`advertised command listed as a skill: ${name}`);
  return {
    kind: "skills-listed",
    observed: {
      listed: bound.skills.map((row) => row.name).toSorted(),
      errors: bound.errors.length,
    },
  };
}

function userMessages(items: readonly NativeThreadItem[]): NativeThreadItem[] {
  return items.filter((item) => item.type === "userMessage");
}

function exactSingleText(item: NativeThreadItem, text: string): boolean {
  const [only, ...extra] = item.content ?? [];
  return only !== undefined && extra.length === 0 && only.type === "text" && only.text === text;
}

function userTexts(thread: NativeThread): string[] {
  return thread.turns.flatMap((turn) =>
    userMessages(turn.items).flatMap((item) =>
      (item.content ?? []).flatMap((input) => (input.text === undefined ? [] : [input.text])),
    ),
  );
}

function checkEffectView(view: NativeThread, submission: SkillSubmission): string {
  const effectTurns = view.turns.filter((turn) =>
    turn.items.some(
      (item) =>
        item.type === "agentMessage" && (item.text ?? "").includes(submission.effect_marker),
    ),
  );
  if (effectTurns.length === 0) throw new Error("skill effect absent from Codex history");
  if (effectTurns.length > 1) throw new Error("skill effect repeated across turns");
  const [turn] = effectTurns;
  if (turn === undefined) throw new Error("unreachable: counted effect turn missing");
  if (turn.status !== "completed") throw new Error("skill turn did not complete");
  const users = userMessages(turn.items);
  if (submission.image_count === 0) {
    const [user, ...extra] = users;
    if (user === undefined || extra.length > 0 || !exactSingleText(user, submission.original_text))
      throw new Error("history user item is not the exact original $name input");
  } else if (users.length > 0) {
    throw new Error("image input synthesized a user history item");
  }
  for (const name of submission.selected)
    if (
      !submission.original_text.includes(`/${name}`) &&
      userTexts(view).some((text) => text.includes(`/${name}`))
    )
      throw new Error("prepared provider invocation leaked into user history");
  return turn.id;
}

export function checkSkillEffect(
  readback: JourneyReadback,
  submission: SkillSubmission,
): JourneyCheckResult {
  const readTurn = checkEffectView(readback.threadRead, submission);
  const resumeTurn = checkEffectView(readback.threadResume, submission);
  if (readTurn !== resumeTurn) throw new Error("read and resume disagree on the skill turn");
  if (!readback.providerAgentTexts.some((text) => text.includes(submission.effect_marker)))
    throw new Error("provider events lack the skill effect");
  return { kind: "skill-effect", observed: { turn_id: readTurn } };
}

function compactionItems(items: readonly NativeThreadItem[]): NativeThreadItem[] {
  return items.filter((item) => item.type === "contextCompaction");
}

function checkCompactedView(view: NativeThread): string {
  const turn = view.turns.at(-1);
  if (turn === undefined) throw new Error("no compaction turn in Codex history");
  if (compactionItems(turn.items).length !== 1)
    throw new Error("last turn lacks exactly one contextCompaction item");
  if (turn.status !== "completed") throw new Error("compaction turn did not complete");
  if (userTexts({ id: view.id, turns: [turn] }).some((text) => text.startsWith("/compact")))
    throw new Error("prepared /compact prompt leaked into user history");
  return turn.id;
}

export function checkCompacted(readback: JourneyReadback): JourneyCheckResult {
  const readTurn = checkCompactedView(readback.threadRead);
  if (readTurn !== checkCompactedView(readback.threadResume))
    throw new Error("read and resume disagree on the compaction turn");
  if (
    !readback.providerToolItems.some(
      (item) => item.toolKind === "contextCompaction" && item.status === "completed",
    )
  )
    throw new Error("provider events lack a completed typed compaction marker");
  return { kind: "compacted", observed: { turn_id: readTurn } };
}

export function checkCompactRefused(
  readback: JourneyReadback,
  before: NativeThread,
): JourneyCheckResult {
  const shape = (thread: NativeThread): string =>
    JSON.stringify(thread.turns.map((turn) => [turn.id, turn.status, turn.items.length]));
  for (const view of [readback.threadRead, readback.threadResume])
    if (shape(view) !== shape(before)) throw new Error("refused compact changed Codex history");
  if (readback.providerToolItems.some((item) => item.toolKind === "contextCompaction"))
    throw new Error("refused compact reached the provider");
  return { kind: "compact-refused", observed: { turns: before.turns.length } };
}

export function checkEffectiveModel(
  readback: JourneyReadback,
  expectedModel: string,
): JourneyCheckResult {
  if (readback.resumedModel !== expectedModel)
    throw new Error("effective model differs from the TUI selection");
  return { kind: "effective-model", observed: { model: readback.resumedModel } };
}

export function checkJourney(
  readback: JourneyReadback,
  target: { readonly endpointId: string; readonly sessionId: string; readonly cwd: string },
  checks: readonly JourneyCheck[],
  before: (path: string) => NativeThread,
): JourneyCheckResult[] {
  return checks.map((check) => {
    switch (check.kind) {
      case "skills-listed":
        return checkSkillsListed(readback, target, check.expected_skills, check.excluded_names);
      case "skill-effect":
        return checkSkillEffect(readback, check.submission);
      case "compacted":
        return checkCompacted(readback);
      case "compact-refused":
        return checkCompactRefused(readback, before(check.before_thread_read));
      case "effective-model":
        return checkEffectiveModel(readback, check.expected_model);
    }
  });
}
