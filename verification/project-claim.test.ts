import assert from "node:assert/strict";
import { test } from "node:test";
import { z } from "zod";
import { claimProject } from "./clients/project-claim.ts";
import { withMcpFixture } from "./test-support/mcp-fixture.ts";

const projectId = "019a0000-0000-7000-8000-000000000015";
const requestId = "019a0000-0000-7000-8000-0000000000ff";
const requestSchema = z.object({
  params: z.object({
    name: z.literal("project_claim"),
    arguments: z
      .object({
        protocolVersion: z.literal(1n),
        projectId: z.literal(projectId),
        requestId: z.literal(requestId),
      })
      .strict(),
  }),
});
const lease = { epoch: 2n, expiresAt: 123456789n, start: { epoch: 1n, sequence: 73n } };
test("claim driver waits through Pending using the identical actor-free attempt", async () => {
  const attempts: string[] = [];
  await withMcpFixture({
    reply: (request) => {
      const parsed = requestSchema.parse(request).params.arguments;
      attempts.push(parsed.requestId);
      return attempts.length === 1
        ? { kind: "pending", value: { attempt: requestId } }
        : { kind: "claimed", value: lease };
    },
    verify: async (client): Promise<void> => {
      assert.deepEqual(
        await claimProject({ client, projectId, requestId, deadline: "2099-01-01T00:00:00Z" }),
        lease,
      );
      assert.deepEqual(attempts, [requestId, requestId]);
    },
  });
});
test("claim driver refuses a pending answer for another attempt", async () => {
  await withMcpFixture({
    reply: (request) => {
      requestSchema.parse(request);
      return { kind: "pending", value: { attempt: projectId } };
    },
    verify: async (client): Promise<void> => {
      await assert.rejects(
        claimProject({ client, projectId, requestId, deadline: "2099-01-01T00:00:00Z" }),
        /same claim attempt/,
      );
    },
  });
});
test("claim driver preserves an explicit refusal instead of treating it as a grant", async () => {
  await withMcpFixture({
    reply: (request) => {
      requestSchema.parse(request);
      return { kind: "refused", value: { kind: "alreadyHeld", value: { holder: projectId } } };
    },
    verify: async (client): Promise<void> => {
      await assert.rejects(
        claimProject({ client, projectId, requestId, deadline: "2099-01-01T00:00:00Z" }),
        /project claim refused/,
      );
    },
  });
});
test("claim driver leaves an expired observation unconfirmed without submitting", async () => {
  let requests = 0;
  await withMcpFixture({
    reply: () => {
      requests += 1;
      return { kind: "claimed", value: lease };
    },
    verify: async (client): Promise<void> => {
      await assert.rejects(
        claimProject({ client, projectId, requestId, deadline: "2000-01-01T00:00:00Z" }),
        /claim observation deadline/,
      );
      assert.equal(requests, 0);
    },
  });
});
