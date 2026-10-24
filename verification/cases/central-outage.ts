import { z } from "zod";
import type { SessionGate } from "../access/session-gate.ts";
import { setTimeout } from "node:timers/promises";
import {
  acceptedWrite,
  boardIdentity,
  writeHeader,
  recordPages,
  recordPageSchema,
  type RecordPage,
} from "../clients/board-requests.ts";
import { field } from "../exact-json.ts";
import type { CaseRegistration } from "../coverage.ts";
import { ObservationOpen, ObservationFailure, type Receipt } from "../receipt.ts";
import type { JourneyMachine } from "../run-context.ts";
import { submitRemoteMessage, finishAndRepeatRemoteMessage } from "./direct-messages.ts";
import { notifyAndRepeat } from "./notifications.ts";

export const centralOutageCase = {
  id: "central-outage-local-independence",
  rows: ["OB4:2", "OB4:W", "OB4:L"],
  evidence: "journey",
} satisfies CaseRegistration;
const workspaceSchema = z.object({
  workspaceId: z.string(),
  name: z.string(),
  description: z.string(),
  state: z.enum(["active", "archiving", "archived"]),
});
const workspaceInputSchema = workspaceSchema.omit({ state: true });
async function workspaceRecords(
  source: JourneyMachine,
  workspace: string,
): Promise<readonly z.infer<typeof workspaceSchema>[]> {
  const records = await recordPages({
    client: source.installation.mcp,
    tool: "workspace_list",
    arguments: { repository: null },
    page: (reply): RecordPage => z.object({ page: recordPageSchema }).parse(reply).page,
  });
  return records
    .map((record) => workspaceSchema.parse(record))
    .filter((record): boolean => record.workspaceId === workspace);
}
async function workspacePresent(input: {
  machine: JourneyMachine;
  expected: z.infer<typeof workspaceInputSchema>;
  receipt: Receipt;
}): Promise<void> {
  const deadline = Date.now() + 30_000;
  do {
    const records = await workspaceRecords(input.machine, input.expected.workspaceId);
    if (records.length > 0) {
      const expected = {
        workspaceId: input.expected.workspaceId,
        name: input.expected.name,
        description: input.expected.description,
        state: "active",
      };
      await input.receipt.equal("workspace effect is the literal record exactly once", records, [
        expected,
      ]);
      const shown = z.object({ workspace: workspaceSchema }).parse(
        await input.machine.installation.mcp.call("workspace_show", {
          workspaceId: expected.workspaceId,
        }),
      );
      await input.receipt.equal(
        "workspace show retains the literal effect",
        shown.workspace,
        expected,
      );
      return;
    }
    await setTimeout(200);
  } while (Date.now() < deadline);
  throw new ObservationFailure(
    "accepted workspace never became visible through the public fleet view",
  );
}
export async function centralOutage(input: {
  source: JourneyMachine;
  target: JourneyMachine;
  edge: SessionGate;
  receipt: Receipt;
}): Promise<void> {
  const { source, target, receipt } = input;
  const workspace = boardIdentity();
  const request = {
    ...writeHeader(source.actor),
    workspaceId: workspace,
    name: workspace,
    description: "Disposable release verification pending request",
  };
  const healthy = {
    ...request,
    ...writeHeader(source.actor),
    workspaceId: boardIdentity(),
    name: boardIdentity(),
  };
  await acceptedWrite({
    client: source.installation.mcp,
    tool: "workspace_create",
    request: healthy,
  });
  for (const machine of [source, target])
    await workspacePresent({ machine, expected: healthy, receipt });
  const messageInput = {
    sender: source.installation,
    senderSession: source.recipient.target,
    recipient: target.recipient,
    receipt,
  };
  const beforeOutage = await input.edge.ready();
  input.edge.pause();
  let submission;
  try {
    const pending = await source.installation.mcp.call("workspace_create", request);
    await receipt.equal(
      "fleet request waits while central is unreachable",
      field(pending, "result", "kind"),
      "pendingUnreachable",
    );
    await receipt.equal(
      "pending workspace is absent from the public fleet cache",
      await workspaceRecords(source, workspace),
      [],
    );
    const before = target.recipient.position();
    submission = await submitRemoteMessage(messageInput);
    const shown = await source.installation.mcp.call("router_show", {
      caller: source.recipient.target,
      reference: submission.admission.item,
    });
    await receipt.equal(
      "message remains queued without central custody",
      field(shown, "mail", "status", "kind"),
      "queued",
    );
    await target.recipient.absent(submission.admission.item, 3000, before);
    await notifyAndRepeat({
      source: source.installation,
      actor: source.actor,
      project: source.heldProject,
      recipients: [source.recipient],
      receipt,
    });
  } finally {
    input.edge.resume();
  }
  await receipt.record("reconnectedSessionWelcome", await input.edge.ready(beforeOutage.welcomes));
  if (submission === undefined)
    throw new ObservationOpen("outage did not produce an observed message submission");
  const decided = await acceptedWrite({
    client: source.installation.mcp,
    tool: "workspace_create",
    request,
  });
  for (const machine of [source, target])
    await workspacePresent({ machine, expected: request, receipt });
  await receipt.equal(
    "original fleet request drains to the same decision",
    await acceptedWrite({
      client: source.installation.mcp,
      tool: "workspace_create",
      request,
    }),
    decided,
  );
  for (const machine of [source, target])
    await workspacePresent({ machine, expected: request, receipt });
  await finishAndRepeatRemoteMessage(messageInput, submission);
}
