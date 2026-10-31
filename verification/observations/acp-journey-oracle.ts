import { createHash } from "node:crypto";
import { INFORMATION_REFUSALS } from "./acp-journey-models.ts";
import type {
  ActiveJourneyReadback,
  ExpectedSkill,
  JourneyCheck,
  JourneyCheckResult,
  JourneyReadback,
  ModelReadback,
  NativeSkillsList,
  NativeThread,
  NativeThreadItem,
  SkillSubmission,
} from "./acp-journey-models.ts";

/**
 * The server-owned selection reference Program Design fixes for provider skills:
 * `/__codex_router_skill__/<endpoint>/<encoded-session>/<encoded-name>`, where session ID and
 * invocation name are each UTF-8 base64url without padding.
 */
export function skillReferencePath(
  endpointId: string,
  sessionId: string,
  invocationName: string,
): string {
  const encode = (value: string): string => Buffer.from(value, "utf8").toString("base64url");
  return `/__codex_router_skill__/${endpointId}/${encode(sessionId)}/${encode(invocationName)}`;
}

/** RFC 4122 name-based UUID (version 5, SHA-1) as canonical lowercase hyphenated text. */
export function uuidV5(namespace: string, name: string): string {
  const digest = createHash("sha1")
    .update(Buffer.from(namespace.replaceAll("-", ""), "hex"))
    .update(name, "utf8")
    .digest()
    .subarray(0, 16);
  digest.writeUInt8((digest.readUInt8(6) & 0x0f) | 0x50, 6);
  digest.writeUInt8((digest.readUInt8(8) & 0x3f) | 0x80, 8);
  const hex = digest.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

const RFC4122_NAMESPACE_URL = "6ba7b811-9dad-11d1-80b4-00c04fd430c8";
// The four text forms the uuid crate's `Uuid::parse_str` accepts: simple, hyphenated, braced, urn.
// Hex digits are case-insensitive; the `urn:uuid:` prefix is matched byte for byte (uuid 1.26.0).
const HEX = "[0-9a-fA-F]";
const HYPHENATED = `(${HEX}{8})-(${HEX}{4})-(${HEX}{4})-(${HEX}{4})-(${HEX}{12})`;
const UUID_FORMS = [
  new RegExp(`^(${HEX}{8})(${HEX}{4})(${HEX}{4})(${HEX}{4})(${HEX}{12})$`, "u"),
  new RegExp(`^${HYPHENATED}$`, "u"),
  new RegExp(`^\\{${HYPHENATED}\\}$`, "u"),
  new RegExp(`^urn:uuid:${HYPHENATED}$`, "u"),
];

/**
 * The native thread ID Codex sees for a provider Session: a raw session ID that is already a UUID
 * in canonical form, otherwise UUIDv5(URL namespace, "codex-router:session:<service>:<endpoint>:<raw>").
 */
export function nativeThreadAlias(
  serviceId: string,
  endpointId: string,
  sessionId: string,
): string {
  for (const form of UUID_FORMS) {
    const uuid = form.exec(sessionId);
    if (uuid !== null) return uuid.slice(1).join("-").toLowerCase();
  }
  return uuidV5(
    RFC4122_NAMESPACE_URL,
    `codex-router:session:${serviceId}:${endpointId}:${sessionId}`,
  );
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

type OrdinaryReplyCheck = Extract<JourneyCheck, { kind: "ordinary-reply" }>;

/** Whether one line of this text, apart from its surrounding whitespace, is exactly the completion. */
function holdsCompletionLine(text: string, completion: string): boolean {
  return text.split(/\r?\n/u).some((line) => line.trim() === completion);
}

/**
 * The reply is the latest turn: completed, holding exactly one user item that is the original
 * prompt as one Text input, and an assistant message with one line that is the whole completion,
 * so the marker inside other prose, such as an error, never counts. No earlier turn holds the
 * completion anywhere, so a prior reply cannot satisfy it, and a user item never stands in for the
 * assistant's text.
 */
function checkReplyView(view: NativeThread, check: OrdinaryReplyCheck): string {
  const turn = view.turns.at(-1);
  if (turn === undefined) throw new Error("no reply turn in Codex history");
  const agentTexts = (items: readonly NativeThreadItem[]): string[] =>
    items.flatMap((item) => (item.type === "agentMessage" ? [item.text ?? ""] : []));
  if (
    view.turns
      .slice(0, -1)
      .some((earlier) => agentTexts(earlier.items).some((text) => text.includes(check.completion)))
  )
    throw new Error("an earlier turn already holds the completion");
  if (!agentTexts(turn.items).some((text) => holdsCompletionLine(text, check.completion)))
    throw new Error("latest turn lacks the assistant completion line");
  if (turn.status !== "completed") throw new Error("reply turn did not complete");
  const [user, ...extra] = userMessages(turn.items);
  if (user === undefined || extra.length > 0 || !exactSingleText(user, check.prompt))
    throw new Error("reply turn user item is not the exact original prompt");
  return turn.id;
}

export function checkOrdinaryReply(
  readback: JourneyReadback,
  check: OrdinaryReplyCheck,
): JourneyCheckResult {
  const readTurn = checkReplyView(readback.threadRead, check);
  if (readTurn !== checkReplyView(readback.threadResume, check))
    throw new Error("read and resume disagree on the reply turn");
  if (!readback.providerAgentTexts.some((text) => holdsCompletionLine(text, check.completion)))
    throw new Error("provider events lack the completion line");
  return { kind: "ordinary-reply", observed: { turn_id: readTurn } };
}

type InformationRefusedCheck = Extract<JourneyCheck, { kind: "information-refused" }>;

/**
 * The observer sent the command's native request exactly once on its own bound connection, the
 * endpoint answered that request ID with its declared code and message, and a supported read on
 * the same connection still returned this thread.
 */
export function checkInformationRefused(
  readback: JourneyReadback,
  check: InformationRefusedCheck,
): JourneyCheckResult {
  const information = readback.information;
  if (information === null)
    throw new Error("information-refused needs the observer's refused request");
  const expected = INFORMATION_REFUSALS[check.command];
  const sent = information.refusals.filter((refusal) => refusal.command === check.command);
  const [refusal, ...repeated] = sent;
  if (refusal === undefined || repeated.length > 0)
    throw new Error(`expected exactly one ${expected.method} request`);
  if (refusal.method !== expected.method)
    throw new Error(`the ${check.command} request used ${refusal.method}`);
  if (refusal.error.code !== expected.code)
    throw new Error(
      `${expected.method} refused with code ${refusal.error.code}, not ${expected.code}`,
    );
  if (refusal.error.message !== expected.message)
    throw new Error(`${expected.method} refusal message differs from the endpoint's declared text`);
  if (information.threadAfter.id !== readback.threadRead.id)
    throw new Error("the supported read after the refusal returned another thread");
  return {
    kind: "information-refused",
    observed: {
      method: refusal.method,
      request_id: String(refusal.requestId),
      code: Number(refusal.error.code),
      message: refusal.error.message,
    },
  };
}

function checkCompactedView(view: NativeThread): string {
  const turn = view.turns.at(-1);
  if (turn === undefined) throw new Error("no compaction turn in Codex history");
  if (compactionItems(turn.items).length !== 1)
    throw new Error("last turn lacks exactly one contextCompaction item");
  if (turn.status !== "completed") throw new Error("compaction turn did not complete");
  if (userTexts({ ...view, turns: [turn] }).some((text) => text.startsWith("/compact")))
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

type ModelCatalogueCheck = Extract<JourneyCheck, { kind: "model-catalogue" }>;

function providerOption(readback: ModelReadback, category: "model" | "effort") {
  return readback.providerCatalog.configOptions.find((option) => option.category === category);
}

function selectValue(option: {
  currentValue: { kind: string; value: string | boolean };
}): string | null {
  return option.currentValue.kind === "select" && typeof option.currentValue.value === "string"
    ? option.currentValue.value
    : null;
}

const sameMembers = (left: readonly string[], right: readonly string[]): boolean =>
  left.length === right.length &&
  [...left].sort().join("\u0000") === [...right].sort().join("\u0000");

/**
 * The bound native catalogue keeps every provider model and adds only the declared delegation; the
 * bootstrap catalogue is the delegation alone; the selected model's controls and current values
 * match the provider Session's own catalogue. The native list is never its own reference.
 */
export function checkModelCatalogue(
  readback: JourneyReadback,
  check: ModelCatalogueCheck,
): JourneyCheckResult {
  const models = readback.models;
  if (models === null) throw new Error("model-catalogue needs the observer's model reads");
  const providerModel = providerOption(models, "model");
  if (providerModel === undefined) throw new Error("the provider Session exposes no model option");
  const providerModels = providerModel.choices.map((choice) => choice.value);
  const boundModels = models.boundModels.data.map((row) => row.model);
  const hidden = providerModels.filter((model) => !boundModels.includes(model));
  if (hidden.length > 0)
    throw new Error(`native catalogue hides provider models: ${hidden.join(", ")}`);
  const extra = boundModels.filter(
    (model) => !providerModels.includes(model) && !check.delegation_models.includes(model),
  );
  if (extra.length > 0) throw new Error(`native catalogue adds models: ${extra.join(", ")}`);
  const bootstrap = models.unboundModels.data.map((row) => row.model);
  if (bootstrap.length === 0 || bootstrap.some((model) => !check.delegation_models.includes(model)))
    throw new Error("bootstrap catalogue is not the delegation alone");
  if (selectValue(providerModel) !== check.selected_model)
    throw new Error("provider current model differs from the selection");
  if (readback.resumedModel !== check.selected_model)
    throw new Error("native current model differs from the provider's");
  const row = models.boundModels.data.find((candidate) => candidate.model === check.selected_model);
  if (row === undefined) throw new Error("selected model has no native row");
  const nativeEfforts = row.supportedReasoningEfforts.map((option) => option.reasoningEffort);
  const providerEffort = providerOption(models, "effort");
  const providerCurrentEffort = providerEffort === undefined ? null : selectValue(providerEffort);
  if (check.effort.kind === "qualified") {
    if (providerEffort === undefined)
      throw new Error("the provider Session exposes no effort option");
    const providerEfforts = providerEffort.choices.map((choice) => choice.value);
    if (!sameMembers(nativeEfforts, providerEfforts))
      throw new Error("native effort choices differ from the provider's options");
    if (!providerEfforts.includes(row.defaultReasoningEffort))
      throw new Error("native default effort is not an offered provider value");
    if (providerCurrentEffort !== check.effort.expected_current)
      throw new Error("provider current effort differs from the selection");
  } else {
    if (
      nativeEfforts.length !== 1 ||
      nativeEfforts[0] !== check.effort.action ||
      row.defaultReasoningEffort !== check.effort.action
    )
      throw new Error("selected model lacks the explicit provider-settings action");
    // The action token is fabricated as an effective effort only when the provider's own current
    // effort is something else; a provider that truly reports the same string (the reserved-value
    // collision, Program Design model chapter) is matching upstream data, decided below.
    if (
      models.resumedEffort === check.effort.action &&
      providerCurrentEffort !== check.effort.action
    )
      throw new Error("the provider-settings action is reported as an effective effort");
  }
  if (models.resumedEffort !== providerCurrentEffort)
    throw new Error("native current effort differs from the provider's");
  return {
    kind: "model-catalogue",
    observed: {
      selected_model: check.selected_model,
      provider_models: providerModels.length,
      native_models: boundModels.length,
      bootstrap_models: bootstrap,
      effort: check.effort.kind,
      native_efforts: nativeEfforts,
      current_effort: providerCurrentEffort,
    },
  };
}

export function checkTurnInProgress(readback: ActiveJourneyReadback): JourneyCheckResult {
  const turn = readback.threadRead.turns.at(-1);
  if (turn === undefined || turn.status !== "inProgress")
    throw new Error("no in-progress turn: the active observation ran after the turn ended");
  return { kind: "turn-in-progress", observed: { turn_id: turn.id } };
}

export function checkActiveJourney(
  readback: ActiveJourneyReadback,
  checks: readonly JourneyCheck[],
): JourneyCheckResult[] {
  return checks.map((check) => {
    if (check.kind !== "turn-in-progress")
      throw new Error(`${check.kind} needs an idle-bind observation`);
    return checkTurnInProgress(readback);
  });
}

export function checkIdleJourney(
  readback: JourneyReadback,
  target: { readonly endpointId: string; readonly sessionId: string; readonly cwd: string },
  checks: readonly JourneyCheck[],
  before: (path: string) => NativeThread,
): JourneyCheckResult[] {
  return checks.map((check) => {
    switch (check.kind) {
      case "turn-in-progress":
        throw new Error("turn-in-progress needs an active-read-only observation");
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
      case "model-catalogue":
        return checkModelCatalogue(readback, check);
      case "ordinary-reply":
        return checkOrdinaryReply(readback, check);
      case "information-refused":
        return checkInformationRefused(readback, check);
    }
  });
}
