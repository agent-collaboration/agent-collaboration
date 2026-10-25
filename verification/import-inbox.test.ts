import { withMcpFixture } from "./test-support/mcp-fixture.ts";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { z } from "zod";
import { identitySchema } from "./declaration.ts";
import { compareLegacyInbox } from "./clients/import-inbox.ts";
import { legacyOracleSchema, type LegacyOracle } from "./clients/legacy-oracle.ts";
import type { RouterMcp } from "./clients/router-mcp.ts";
import type { Receipt } from "./receipt.ts";

const project = "019a0000-0000-7000-8000-000000000015";
const foreign = "019a0000-0000-7000-8000-000000000016";
const reader = identitySchema.parse({ kind: "human", humanId: "synthetic-reader" });
const requestSchema = z.object({
  id: z.string(),
  method: z.literal("tools/call"),
  params: z.object({
    name: z.literal("inbox_fetch"),
    arguments: z.object({
      protocolVersion: z.literal(1n),
      actingFor: z.null(),
      reader: identitySchema,
      scope: z.object({ kind: z.literal("project"), projectId: z.literal(project) }),
      readMode: z.enum(["latest", "unread"]),
      initializations: z.array(z.never()),
      page: z.object({ limit: z.literal(100n), cursor: z.string().nullable() }),
    }),
  }),
});
type InboxRequest = z.infer<typeof requestSchema>["params"]["arguments"];
function oracle(mode: "latest" | "unread", ids: readonly string[]): LegacyOracle {
  return legacyOracleSchema.parse({
    kind: "legacyOracleOnly",
    product_import_observed: false,
    source_sha256: "0".repeat(64),
    driver_sha256: "1".repeat(64),
    activity_count: 3n,
    projects: [],
    selections: [
      {
        reader: "source-key",
        legacy_topic: project,
        mode,
        post_ids: ids,
        post_count: BigInt(ids.length),
        covered_post_set_sha256: createHash("sha256")
          .update(JSON.stringify([...ids].sort()))
          .digest("hex"),
      },
    ],
  });
}
function page(
  request: InboxRequest,
  records: readonly unknown[],
  nextCursor: string | null,
): unknown {
  return {
    page: {
      scope: request.scope,
      reader: request.reader,
      readMode: request.readMode,
      records,
      nextCursor,
      unavailable: [],
      initializations: [],
    },
  };
}
function post(id: string, projectId = project): unknown {
  return { kind: "postCreated", post: { postId: id, projectId } };
}
async function withMcp(input: {
  reply(request: InboxRequest): unknown;
  verify(client: RouterMcp, receipt: Receipt): Promise<void>;
}): Promise<void> {
  await withMcpFixture({
    reply: (request) => input.reply(requestSchema.parse(request).params.arguments),
    verify: input.verify,
  });
}
function input(
  client: RouterMcp,
  receipt: Receipt,
  expected: LegacyOracle,
): Parameters<typeof compareLegacyInbox>[0] {
  return {
    client,
    receipt,
    oracle: expected,
    readers: new Map([["source-key", reader]]),
    deadline: "2099-01-01T00:00:00Z",
  };
}
test("import comparison exhausts pages and permits foreign watched context without crediting it", async () => {
  await withMcp({
    reply: (request) =>
      request.page.cursor === null
        ? page(
            request,
            [post("foreign", foreign), post("second"), { kind: "threadStateChanged" }],
            "next",
          )
        : page(request, [post("first")], null),
    verify: async (client, receipt): Promise<void> => {
      assert.deepEqual(
        await compareLegacyInbox(input(client, receipt, oracle("latest", ["first", "second"]))),
        { comparisons: 1, nonempty: 1 },
      );
    },
  });
});
test("import comparison accepts an empty unread set without initializing its bookmark", async () => {
  await withMcp({
    reply: (request) => page(request, [], null),
    verify: async (client, receipt): Promise<void> => {
      assert.deepEqual(await compareLegacyInbox(input(client, receipt, oracle("unread", []))), {
        comparisons: 1,
        nonempty: 0,
      });
    },
  });
});
test("import comparison rejects an extra selected post even when expected posts are present", async () => {
  await withMcp({
    reply: (request) => page(request, [post("first"), post("extra")], null),
    verify: async (client, receipt): Promise<void> => {
      await assert.rejects(
        compareLegacyInbox(input(client, receipt, oracle("latest", ["first"]))),
        /covered post set/,
      );
    },
  });
});
test("import comparison refuses a repeated continuation instead of silently truncating", async () => {
  await withMcp({
    reply: (request) => page(request, [post("first")], "same"),
    verify: async (client, receipt): Promise<void> => {
      await assert.rejects(
        compareLegacyInbox(input(client, receipt, oracle("latest", ["first"]))),
        /continuation repeated/,
      );
    },
  });
});
test("import comparison rejects a different reader even when its post set matches", async () => {
  await withMcp({
    reply: (request) => ({
      page: {
        scope: request.scope,
        reader: { kind: "human", humanId: "wrong-reader" },
        readMode: request.readMode,
        records: [post("first")],
        nextCursor: null,
        unavailable: [],
        initializations: [],
      },
    }),
    verify: async (client, receipt): Promise<void> => {
      await assert.rejects(
        compareLegacyInbox(input(client, receipt, oracle("latest", ["first"]))),
        /selected reader, project and mode/,
      );
    },
  });
});
test("import comparison keeps an unavailable selected project unconfirmed", async () => {
  await withMcp({
    reply: (request) => ({
      page: {
        scope: request.scope,
        reader: request.reader,
        readMode: request.readMode,
        records: [],
        nextCursor: null,
        unavailable: [project],
        initializations: [],
      },
    }),
    verify: async (client, receipt): Promise<void> => {
      await assert.rejects(
        compareLegacyInbox(input(client, receipt, oracle("unread", []))),
        /selected imported project is unavailable/,
      );
    },
  });
});
