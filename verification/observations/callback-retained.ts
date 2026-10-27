import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { z } from "zod";
import { evidenceJson } from "../evidence-json.ts";
import { sameJsonContent } from "../json-comparison.ts";
import { interactionSnapshotSchema, observedObject } from "./evidence-models.ts";
import { readSnapshot, requireCallbackCapture } from "./interaction-readback.ts";
import { journeyRecord, type JourneyPlan } from "./provider-journey-plan.ts";
import { saveRecord } from "./runtime-plan.ts";

export function verifyRetained(plan: JourneyPlan): void {
  const operation = journeyRecord(plan, "operation-terminal");
  const capture = journeyRecord(plan, "callback-events");
  const answerArguments = observedObject(journeyRecord(plan, "question_answer-intent").arguments);
  const choiceArguments = observedObject(journeyRecord(plan, "approval_decide-intent").arguments);
  const requestIds = [
    z.string().parse(answerArguments.requestId),
    z.string().parse(choiceArguments.requestId),
  ];
  const marker = join(dirname(plan.evidence), "workspace", "automated-approval-marker");
  if (
    operation.stage !== "terminal" ||
    operation.effect !== "applied" ||
    operation.reconciliation !== "confirmed"
  )
    throw new Error("provider operation not settled; do not replay");
  requireCallbackCapture(capture, operation, requestIds);
  if (
    !existsSync(marker) ||
    !statSync(marker).isFile() ||
    !readFileSync(marker).equals(Buffer.from("VERIFICATION_MARKER\n"))
  )
    throw new Error("provider tool did not create exact marker");
  const terminal = readSnapshot(join(plan.service_directory, "interaction.sqlite"), requestIds);
  const answer = observedObject(answerArguments.response).content;
  const choice = z.string().parse(choiceArguments.optionId);
  const view = interactionSnapshotSchema.parse(terminal);
  if (
    !sameJsonContent(view.records[0]?.record.state, { kind: "answered", content: answer }) ||
    !sameJsonContent(view.records[1]?.record.state, { kind: "decided", option_id: choice })
  )
    throw new Error("stored terminal answer/choice differs from actual decisions");
  saveRecord(join(plan.evidence, "terminal-sqlite.json"), terminal);
  saveRecord(join(plan.evidence, "request-ids.json"), requestIds);
  console.log(
    evidenceJson({
      callbackJourneyObserved: true,
      requestIds,
      markerSHA256: createHash("sha256").update(readFileSync(marker)).digest("hex"),
      owningReopenStillRequired: true,
    }),
  );
}
