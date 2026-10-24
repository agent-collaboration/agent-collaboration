import type { CaseRegistration } from "../coverage.ts";
import type { JourneyMachine } from "../run-context.ts";
import type { Receipt } from "../receipt.ts";
import type { SessionGate } from "../access/session-gate.ts";
import { sendAndRepeatRemoteMessage } from "./direct-messages.ts";
import { notifyAndRepeat } from "./notifications.ts";

export const messageNotificationCase = {
  id: "two-machine-message-notification",
  rows: ["OB4:1", "OB4:D", "OB4:M"],
  evidence: "journey",
} satisfies CaseRegistration;
export async function messagesAndNotifications(input: {
  machines: readonly [JourneyMachine, JourneyMachine];
  gates: ReadonlyMap<string, SessionGate>;
  receipt: Receipt;
}): Promise<void> {
  const [first, second] = input.machines;
  await input.receipt.require(
    "two distinct recipient installations",
    first.recipient.target.endpoint.serviceId !== second.recipient.target.endpoint.serviceId,
  );
  for (const [source, target] of [
    [first, second],
    [second, first],
  ]) {
    if (source === undefined || target === undefined) throw new Error("two-machine pair required");
    await input.receipt.equal(
      "sender recipient belongs to its normal Host",
      (await source.installation.identity()).serviceId,
      source.recipient.target.endpoint.serviceId,
    );
    await input.receipt.equal(
      "remote recipient belongs to its normal Host",
      (await target.installation.identity()).serviceId,
      target.recipient.target.endpoint.serviceId,
    );
    const gate = input.gates.get(target.installation.machine.name);
    if (gate === undefined) throw new Error("recipient Session gate absent");
    const connected = await gate.ready();
    await sendAndRepeatRemoteMessage({
      sender: source.installation,
      senderSession: source.recipient.target,
      recipient: target.recipient,
      receipt: input.receipt,
    });
    const observed = await gate.ready();
    await input.receipt.require(
      "connected recipient observes actual items MailWaiting and correlated model arrival",
      observed.mailWaiting.items > connected.mailWaiting.items,
    );
    await input.receipt.record("mailWaitingSignal", {
      machine: target.installation.machine.name,
      before: connected.mailWaiting,
      after: observed.mailWaiting,
      invalidConnections: observed.invalidConnections,
    });
    await notifyAndRepeat({
      source: source.installation,
      actor: source.actor,
      project: source.heldProject,
      recipients: [source, target],
      receipt: input.receipt,
    });
  }
}
