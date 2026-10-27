import { join } from "node:path";
import { arrayField, textField } from "../exact-json.ts";
import { evidenceJson } from "../evidence-json.ts";
import {
  observedObject,
  operationViewSchema,
  providerRecordSchema,
  publicObservationSchema,
  type JsonObject,
} from "./evidence-models.ts";
import {
  approverFor,
  callerFor,
  journeyRecord,
  operationId,
  providerRecord,
  type JourneyPlan,
} from "./provider-journey-plan.ts";
import { callOnce, rpcCall } from "./provider-journey-rpc.ts";
import { qualifyBeforeInput } from "./provider-journey-qualification.ts";
import { saveRecord } from "./runtime-plan.ts";

export { createSession, inspectCreatedSession } from "./provider-session-create.ts";

export async function closeSession(plan: JourneyPlan): Promise<void> {
  const target = journeyRecord(plan, "target-binding");
  const operation = operationId();
  await callOnce(plan, "conversation_close", {
    operationId: operation,
    target,
    requestedBy: callerFor(plan),
    approver: approverFor(plan),
  });
  const settled = await rpcCall(plan, "conversation_operation_wait", {
    operationId: operation,
    timeoutSeconds: 30,
  });
  saveRecord(join(plan.evidence, "close-settlement.json"), settled);
  console.log(evidenceJson(settled));
}

export async function observeSession(
  plan: JourneyPlan,
  timeoutSeconds: number = 1,
): Promise<JsonObject> {
  const target = journeyRecord(plan, "target-binding");
  const observation = await rpcCall(plan, "events_observe", {
    target,
    timeoutSeconds,
    maxEvents: 1024,
    maxBytes: 1048576,
  });
  const view = publicObservationSchema.parse(observation);
  const rawEvents = arrayField(observation, "events");
  const events: JsonObject[] = [];
  for (const [index, wrappedView] of view.events.entries()) {
    const wrapped = observedObject(rawEvents[index]);
    const event = wrappedView.event;
    if (event.kind === "resyncRequired" || event.kind === "eventTooLarge")
      throw new Error("observer resync or oversized event gap");
    if (
      [
        "interactionRequested",
        "interactionResolved",
        "turnStarted",
        "turnEnded",
        "inputAccepted",
      ].includes(String(event.kind))
    )
      events.push(wrapped);
    else if (
      (event.kind === "itemStarted" || event.kind === "itemUpdated") &&
      observedObject(observedObject(event.item).kind).kind === "toolCall"
    ) {
      const item = { ...observedObject(event.item) };
      delete item.text;
      events.push({ ...wrapped, event: { ...event, item } });
    }
  }
  const metadata = { ...observation };
  delete metadata.events;
  const retained = { target, events, observation: metadata };
  saveRecord(join(plan.evidence, "callback-events.json"), retained);
  return retained;
}

export async function initializeApprover(plan: JourneyPlan): Promise<void> {
  await qualifyBeforeInput(plan, false);
  const target = journeyRecord(plan, "target-binding");
  const caller = callerFor(plan);
  const operation = operationId();
  const text =
    "You are a dedicated test-automation notification sink on this debug Host. For every future Router Question or Approval notice, call no tools, make no decision, change nothing, and reply exactly TEST_NOTICE_ACK. An external test driver will answer the actual requests explicitly under your real Session identity. For this initialization only, call no tools and reply exactly TEST_APPROVER_READY, then end the turn.";
  const result = await callOnce(plan, "conversation_prompt", {
    operationId: operation,
    target,
    requestedBy: caller,
    approver: approverFor(plan),
    message: { kind: "agent", sender: caller, text },
    timeoutSeconds: 60,
  });
  const terminal = await rpcCall(plan, "conversation_operation_show", { operationId: operation });
  saveRecord(join(plan.evidence, "initialization-operation.json"), terminal);
  const view = operationViewSchema.parse(terminal);
  if (
    view.stage !== "terminal" ||
    view.effect !== "applied" ||
    view.reconciliation !== "confirmed" ||
    textField(result, "settlement", "detail", "output", "text").trim() !== "TEST_APPROVER_READY"
  )
    throw new Error("approver initialization not confirmed; no requester prompt");
  const capture = await observeSession(plan);
  if (
    publicObservationSchema
      .parse(capture)
      .events.some((wrapped) =>
        ["itemStarted", "itemUpdated", "interactionRequested"].includes(String(wrapped.event.kind)),
      )
  )
    throw new Error("approver initialization used a tool or requested an interaction");
  const inspected = await rpcCall(plan, "provider_session_inspect", { target });
  saveRecord(join(plan.evidence, "approver-ready-inspect.json"), inspected);
  if (providerRecordSchema.parse(providerRecord(inspected)).state !== "idle")
    throw new Error("approver not idle after initialization");
  console.log(evidenceJson({ actualApprover: target, noToolsInitializationConfirmed: true }));
}
