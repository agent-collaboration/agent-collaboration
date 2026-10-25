import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { sessionSchema } from "../declaration.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { JourneyMachine } from "../run-context.ts";
import { wakeSnapshotSchema } from "./remote-wake-inputs.ts";

const deliverySchema = z.object({
  deliveryId: z.string(),
  target: sessionSchema,
  source: z.object({ kind: z.literal("wake"), wakeupId: z.string(), occurrenceId: z.string() }),
  disposition: z.string(),
  evidence: z.unknown(),
});
export async function observeWakeFire(input: {
  target: JourneyMachine;
  wakeup: string;
  marker: string;
  receipt: Receipt;
  seconds: number;
}): Promise<string> {
  const deadline = Date.now() + input.seconds * 1000;
  do {
    const snapshot = wakeSnapshotSchema.parse(
      await input.target.installation.mcp.call("wake_show", { wakeupId: input.wakeup }),
    );
    const page = z
      .object({ records: z.array(deliverySchema), nextCursor: z.string().nullable() })
      .parse(
        await input.target.installation.mcp.call("delivery_list", {
          wakeupId: input.wakeup,
          cursor: null,
          limit: 100n,
        }),
      );
    if (page.nextCursor !== null)
      throw new ObservationOpen("wake delivery inventory is incomplete");
    if (snapshot.firstFire !== null) {
      await input.receipt.equal(
        "first fire belongs to B's own wake",
        snapshot.firstFire.wakeupId,
        input.wakeup,
      );
      const matches = page.records.filter(
        (delivery): boolean => delivery.source.occurrenceId === snapshot.firstFire?.occurrenceId,
      );
      if (matches.length > 1) throw new Error("wake occurrence produced multiple deliveries");
      const delivery = matches[0];
      if (delivery?.disposition === "accepted") {
        await input.receipt.equal(
          "wake delivery stays on the intended installation",
          delivery.target,
          input.target.recipient.target,
        );
        await input.receipt.equal(
          "wake delivery names the actual timer",
          delivery.source.wakeupId,
          input.wakeup,
        );
        const accepted = z
          .object({
            kind: z.literal("accepted"),
            receipt: z.object({ client: z.object({ clientUserMessageId: z.string() }) }),
          })
          .parse(delivery.evidence);
        const clientId = accepted.receipt.client.clientUserMessageId;
        await input.target.recipient.completed(clientId, input.marker);
        await input.target.recipient.unique(clientId, 3000);
        await input.receipt.record("remoteWakeRecipient", {
          wakeup: input.wakeup,
          occurrence: snapshot.firstFire.occurrenceId,
          delivery: delivery.deliveryId,
          actualNativeInput: clientId,
          dueAt: snapshot.firstFire.dueAt,
        });
        return clientId;
      }
      if (
        delivery !== undefined &&
        ["failed", "discarded", "uncertain"].includes(delivery.disposition)
      )
        throw new Error("wake fired without known actual recipient acceptance");
    }
    input.target.recipient.connection.healthy();
    await setTimeout(250);
  } while (Date.now() < deadline);
  throw new ObservationOpen("owning wake fire and correlated recipient input were not observed");
}
