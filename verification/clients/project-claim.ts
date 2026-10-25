import { z } from "zod";
import { setTimeout } from "node:timers/promises";
import type { RouterMcp } from "./router-mcp.ts";
import { ObservationFailure, ObservationOpen } from "../receipt.ts";

const claimedSchema = z.object({
  kind: z.literal("claimed"),
  value: z.object({
    epoch: z.bigint().positive(),
    expiresAt: z.bigint(),
    start: z.object({ epoch: z.bigint().positive(), sequence: z.bigint().nonnegative() }),
  }),
});
export type ClaimedProject = z.infer<typeof claimedSchema>["value"];
const replySchema = z.discriminatedUnion("kind", [
  claimedSchema,
  z.object({ kind: z.literal("pending"), value: z.object({ attempt: z.string() }) }),
  z.object({ kind: z.literal("refused"), value: z.unknown() }),
]);
export async function claimProject(input: {
  client: RouterMcp;
  projectId: string;
  requestId: string;
  deadline: string;
}): Promise<ClaimedProject> {
  for (;;) {
    if (Date.now() >= Date.parse(input.deadline))
      throw new ObservationOpen("project claim observation deadline reached");
    const reply = replySchema.parse(
      await input.client.call("project_claim", {
        protocolVersion: 1n,
        projectId: input.projectId,
        requestId: input.requestId,
      }),
    );
    switch (reply.kind) {
      case "claimed":
        return reply.value;
      case "refused":
        throw new ObservationFailure("project claim refused", { cause: reply.value });
      case "pending":
        if (reply.value.attempt !== input.requestId)
          throw new ObservationFailure("pending answer does not retain the same claim attempt");
        await setTimeout(200);
        break;
    }
  }
}
