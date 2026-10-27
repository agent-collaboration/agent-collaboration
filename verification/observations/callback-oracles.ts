import type { CallbackCapture, RequestedInteraction } from "./evidence-models.ts";
import { sameJsonContent } from "../json-comparison.ts";
import {
  callbackCaptureSchema,
  catalogRecordSchema,
  interactionSnapshotSchema,
  observedObject,
  operationViewSchema,
  questionViewSchema,
  readObservedJson,
  type JsonObject,
} from "./evidence-models.ts";

export function requireAdvertisedSettings(
  record: unknown,
  expected: Readonly<Record<string, string>>,
): JsonObject {
  if (Object.keys(expected).length === 0)
    throw new Error("settings not qualified; inspect the actual catalog before model input");
  const catalog = catalogRecordSchema.parse(record).settingsCatalog;
  for (const [name, value] of Object.entries(expected)) {
    if (!["mode", "model", "effort"].includes(name))
      throw new Error("setting has no supported CLI selector");
    let candidates = catalog.configOptions.filter(
      (option) =>
        option.category === name &&
        option.id !== "thinking" &&
        option.currentValue.kind === "select",
    );
    const explicit = candidates.filter((option) => option.id === name);
    if (explicit.length > 0) candidates = explicit;
    let current: string | boolean;
    let choices;
    if (name === "mode" && candidates.length === 0) {
      current = catalog.currentMode;
      choices = catalog.modes;
    } else {
      const candidate = candidates[0];
      if (candidates.length !== 1 || candidate === undefined)
        throw new Error("advertised setting is missing or ambiguous");
      current = candidate.currentValue.value;
      choices = candidate.choices;
    }
    if (current !== value || !choices.some((choice) => choice.value === value))
      throw new Error("actual provider setting differs from selected advertised value");
  }
  return observedObject(observedObject(record).settingsCatalog);
}

export function callbackPrompt(tools: Readonly<Record<string, string>>, command: string): string {
  for (const field of ["question_tool", "question_method", "execution_tool"])
    if (tools[field] === undefined) throw new Error(`qualified tool route missing ${field}`);
  return (
    "This is labelled test automation for typed Question and Approval callbacks. " +
    `First call the actual ${tools.question_tool} tool on its supported ${tools.question_method} route with exactly one single-select question: ` +
    "Which color should this test acknowledge in its final text? Offer exactly Blue and Green. The answer is only a text label; the marker command and its literal contents stay exactly as given below. " +
    "Wait for the actual tool answer; do not merely ask in prose. " +
    `After receiving Blue, call ${tools.execution_tool} exactly once with this exact command: ${command}\n` +
    "The actual Session approver will select allow once for that private marker action. " +
    "Do not call any other tools, delegate, retry, change permissions, or use another path. " +
    "After the tool result, reply exactly QUESTION_BLUE_APPROVAL_CALLBACK_COMPLETE and end the turn."
  );
}

export function decodeRpcBody(body: string): JsonObject {
  let result: JsonObject;
  if (body.trimStart().startsWith("{")) result = readObservedJson(body);
  else {
    const responses = body
      .split(/\r?\n/)
      .filter((line) => line.startsWith("data: "))
      .map((line) => readObservedJson(line.slice(6)))
      .filter((frame) => frame.id === 1n);
    const response = responses[0];
    if (responses.length !== 1 || response === undefined)
      throw new Error("missing or ambiguous correlated RPC response");
    result = response;
  }
  if (result.id !== 1n) throw new Error("wrong RPC response identity");
  return result;
}

export function questionResponseFor(record: unknown): JsonObject {
  const view = questionViewSchema.parse(record);
  const choices = view.fields.filter((field) => field.kind === "singleChoice");
  const choice = choices[0];
  if (choices.length !== 1 || choice === undefined)
    throw new Error("expected one real single-choice field");
  const extras = view.fields.filter((field) => !sameJsonContent(field, choice));
  if (
    extras.some(
      (field) =>
        field.kind !== "text" || field.required || field.fieldId !== `${choice.fieldId}_custom`,
    ) ||
    extras.length > 1
  )
    throw new Error("unexpected additional Question field");
  const blue = choice.options.filter((option) => option.label === "Blue");
  const firstBlue = blue[0];
  if (
    blue.length !== 1 ||
    firstBlue === undefined ||
    choice.options.length !== 2 ||
    !sameJsonContent([...new Set(choice.options.map((option) => option.label))].sort(), [
      "Blue",
      "Green",
    ])
  )
    throw new Error("unexpected Question options");
  return {
    action: "answered",
    content: { [choice.fieldId]: { selectedOptionIds: [firstBlue.optionId] } },
  };
}

function requireOrderedSequences(events: CallbackCapture["events"]): void {
  const sequences = events.map((wrapped) => wrapped.sequence);
  const sorted = [...new Set(sequences)].sort((left, right) =>
    left < right ? -1 : left > right ? 1 : 0,
  );
  if (!sameJsonContent(sequences, sorted))
    throw new Error("duplicate or out-of-order observed event sequences");
}

export function requireCallbackCapture(
  captureInput: unknown,
  operationInput: unknown,
  requestIds: readonly string[],
): void {
  const capture = callbackCaptureSchema.parse(captureInput);
  const operation = operationViewSchema.parse(operationInput);
  const observation = capture.observation;
  if (
    !sameJsonContent(capture.target, operation.target) ||
    !sameJsonContent(observation.target, operation.target) ||
    !sameJsonContent(observation.generation, operation.binding.binding.generation)
  )
    throw new Error("observer target or provider binding generation mismatch");
  if (!observation.attached || observation.endReason !== "deadlineReached")
    throw new Error("observer did not complete its bounded attachment");
  const events = capture.events;
  requireOrderedSequences(events);
  const started = events.flatMap((wrapped) =>
    wrapped.event.kind === "turnStarted" ? [wrapped.event] : [],
  );
  const ended = events.flatMap((wrapped) =>
    wrapped.event.kind === "turnEnded" ? [wrapped.event] : [],
  );
  const start = started[0];
  const end = ended[0];
  if (
    started.length !== 1 ||
    ended.length !== 1 ||
    start === undefined ||
    end === undefined ||
    !start.inputId ||
    !start.turnId ||
    (operation.inputId !== null && start.inputId !== operation.inputId) ||
    end.turnId !== start.turnId ||
    end.outcome.kind !== "ended" ||
    end.outcome.stop_reason !== "endTurn"
  )
    throw new Error("exact input/turn terminal correlation missing");
  const accepted = events.flatMap((wrapped) =>
    wrapped.event.kind === "inputAccepted" ? [wrapped.event] : [],
  );
  if (
    accepted.length > 1 ||
    accepted.some((event) => event.inputId !== start.inputId || event.turnId !== start.turnId)
  )
    throw new Error("unexpected additional requester input");
  const requested = events.flatMap((wrapped) =>
    wrapped.event.kind === "interactionRequested"
      ? [{ sequence: wrapped.sequence, interaction: wrapped.event.interaction }]
      : [],
  );
  const first = requested[0];
  const second = requested[1];
  if (
    first === undefined ||
    second === undefined ||
    !sameJsonContent(
      requested.map((entry) => entry.interaction.kind),
      ["question", "approval"],
    ) ||
    !sameJsonContent(
      requested.map((entry) => entry.interaction.request.requestId),
      requestIds,
    )
  )
    throw new Error("actual Question-first request identities/order missing");
  const resolved = new Map(
    events.flatMap((wrapped): [string, bigint][] =>
      wrapped.event.kind === "interactionResolved"
        ? [[wrapped.event.requestId, wrapped.sequence]]
        : [],
    ),
  );
  const firstId = requestIds[0];
  const secondId = requestIds[1];
  const firstResolution = firstId === undefined ? undefined : resolved.get(firstId);
  const secondResolution = secondId === undefined ? undefined : resolved.get(secondId);
  if (
    !sameJsonContent([...resolved.keys()].sort(), [...new Set(requestIds)].sort()) ||
    firstResolution === undefined ||
    secondResolution === undefined ||
    !(
      first.sequence < firstResolution &&
      firstResolution < second.sequence &&
      second.sequence < secondResolution
    )
  )
    throw new Error("actual interaction resolution order missing");
  requireToolCompletion({
    events,
    approval: second.interaction,
    approvalRequestedSequence: second.sequence,
    approvalResolutionSequence: secondResolution,
  });
}

function requireToolCompletion(input: {
  readonly events: CallbackCapture["events"];
  readonly approval: RequestedInteraction;
  readonly approvalRequestedSequence: bigint;
  readonly approvalResolutionSequence: bigint;
}): void {
  const { events, approval, approvalRequestedSequence, approvalResolutionSequence } = input;
  if (approval.request.subject === null) throw new Error("approval tool subject missing");
  const approvalTool = approval.request.subject.toolCall.toolCallId;
  const toolEvents = events.flatMap((wrapped) =>
    wrapped.event.kind === "itemStarted" || wrapped.event.kind === "itemUpdated"
      ? [{ sequence: wrapped.sequence, item: wrapped.event.item }]
      : [],
  );
  const tools = new Set(toolEvents.map((entry) => entry.item.itemId));
  const completed = new Map(
    toolEvents
      .filter((entry) => entry.item.kind.status === "completed")
      .map((entry) => [entry.item.itemId, entry.sequence]),
  );
  if (
    tools.size !== 2 ||
    !sameJsonContent([...completed.keys()].sort(), [...tools].sort()) ||
    !completed.has(approvalTool)
  )
    throw new Error("actual tool completion missing or unexpected tool");
  const questionTool = [...tools].find((tool) => tool !== approvalTool);
  const questionCompletion = questionTool === undefined ? undefined : completed.get(questionTool);
  const approvalCompletion = completed.get(approvalTool);
  if (
    questionCompletion === undefined ||
    approvalCompletion === undefined ||
    !(questionCompletion < approvalRequestedSequence) ||
    !(approvalResolutionSequence < approvalCompletion)
  )
    throw new Error("actual callback/tool completion order missing");
}

export function compareReopen(before: unknown, after: unknown): JsonObject {
  const view = interactionSnapshotSchema.parse(before);
  if (view.records.length === 0) throw new Error("empty evidence cannot prove reopen");
  for (const item of view.records) {
    const terminal =
      item.record.kind === "question"
        ? "answered"
        : item.record.kind === "approval"
          ? "decided"
          : undefined;
    if (terminal === undefined || item.record.state.kind !== terminal)
      throw new Error("pending or cancelled record cannot prove callback settlement");
  }
  if (!sameJsonContent(before, after))
    throw new Error(
      "terminal content, creation instant, root or revision changed on owning reopen",
    );
  return { supported: true, terminalRecordCount: view.records.length, revision: view.revision };
}
