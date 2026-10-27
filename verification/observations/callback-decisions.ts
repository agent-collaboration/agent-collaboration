import { join } from "node:path";
import { arrayField } from "../exact-json.ts";
import { sameJsonContent } from "../json-comparison.ts";
import { evidenceJson } from "../evidence-json.ts";
import { splitTextLines } from "../text-lines.ts";
import {
  interactionListingSchema,
  interactionSnapshotSchema,
  observedObject,
  publicInteractionSchema,
  type JsonObject,
} from "./evidence-models.ts";
import { questionResponseFor, readSnapshot } from "./interaction-readback.ts";
import { type JourneyPlan } from "./provider-journey-plan.ts";
import { callOnce, rpcCall } from "./provider-journey-rpc.ts";
import { saveRecord } from "./runtime-plan.ts";

export async function scopedRecords(
  plan: JourneyPlan,
  kind: string,
  target: JsonObject,
): Promise<JsonObject[]> {
  const arguments_: JsonObject = { pending: true };
  if (kind === "approval") arguments_.includeOptions = true;
  const raw = await rpcCall(
    plan,
    kind === "question" ? "question_list" : "approval_list",
    arguments_,
  );
  const view = interactionListingSchema.parse(raw);
  const field = kind === "question" ? "questions" : "approvals";
  const records = view[field];
  const rawRecords = raw[field] === undefined ? [] : arrayField(raw, field);
  return records.flatMap((record, index) =>
    sameJsonContent(record.requester, target) ? [observedObject(rawRecords[index])] : [],
  );
}

function approvalArguments(input: {
  readonly pending: JsonObject;
  readonly record: JsonObject;
  readonly actor: JsonObject;
  readonly command: string;
}): JsonObject {
  const request = interactionSnapshotSchema.parse(input.pending).records[0]?.record.request;
  if (request === undefined || request === null || request.subject === null)
    throw new Error("approval request/subject missing; no allow decision");
  if (
    !splitTextLines(request.description ?? "").includes(`command: ${input.command}`) ||
    request.subject.toolCall.kind !== "Execute"
  )
    throw new Error("unexpected approval tool/action; no allow decision");
  const record = publicInteractionSchema.parse(input.record);
  const options = record.options.filter(
    (option) => option.effect === "allow" && option.scope === "once" && !option.persistentTarget,
  );
  const option = options[0];
  if (options.length !== 1 || option === undefined)
    throw new Error("actual allow-once option unavailable");
  return {
    requestId: record.requestId,
    actor: input.actor,
    optionId: option.optionId,
    note: "Labelled test automation: exact private owned marker only",
  };
}

export async function decideCallback(input: {
  readonly plan: JourneyPlan;
  readonly kind: string;
  readonly records: readonly JsonObject[];
  readonly actor: JsonObject;
  readonly command: string;
}): Promise<string> {
  const { plan, kind, records, actor, command } = input;
  const raw = records[0];
  if (
    records.length !== 1 ||
    raw === undefined ||
    !sameJsonContent(publicInteractionSchema.parse(raw).approver, actor)
  )
    throw new Error("ambiguous request or wrong actual Session Approver");
  const record = publicInteractionSchema.parse(raw);
  saveRecord(join(plan.evidence, `${kind}-pending-public.json`), raw);
  const pending = readSnapshot(join(plan.service_directory, "interaction.sqlite"), [
    record.requestId,
  ]);
  saveRecord(join(plan.evidence, `${kind}-pending-sqlite.json`), pending);
  if (kind === "question")
    await callOnce(plan, "question_answer", {
      requestId: record.requestId,
      actor,
      response: questionResponseFor(raw),
    });
  else
    await callOnce(
      plan,
      "approval_decide",
      approvalArguments({ pending, record: raw, actor, command }),
    );
  console.log(
    evidenceJson({
      automatedDecision: kind,
      requestId: record.requestId,
      actor: "actual Session",
      humanImpersonation: false,
    }),
  );
  return record.requestId;
}
