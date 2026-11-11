import { test } from "node:test";
import assert from "node:assert/strict";
import { collectRouterLists } from "./router-lists.ts";
import type { ReadObservation } from "./record.ts";
const workspaceId = "01900000-0000-7000-8000-000000000001",
  boardId = "01900000-0000-7000-8000-000000000002",
  projectId = "01900000-0000-7000-8000-000000000003";
function reply(records: unknown[], nextCursor: string | null = null): ReadObservation {
  return {
    artifact: "/fixture/public-page",
    sha256: "a".repeat(64),
    body: {
      kind: "result",
      cliVersion: "0.1.125",
      serviceVersion: "0.1.125",
      result: { page: { records, nextCursor } },
    },
  };
}
test("lists enumerate archived boards and project parents without carrying opaque envelopes", async (): Promise<void> => {
  const calls: (readonly string[])[] = [];
  const result = await collectRouterLists({
    expectedVersion: "0.1.125",
    read: async (arguments_) => {
      calls.push(arguments_);
      if (arguments_[0] === "workspace")
        return reply([
          {
            workspaceId,
            name: "workspace",
            description: "",
            state: "active",
            repositories: [],
            envelope: { ignoredObservationTime: "now" },
          },
        ]);
      if (arguments_[0] === "board") {
        assert.ok(arguments_.includes("--include-archived"));
        return reply([
          {
            boardId,
            workspaceId,
            name: "archived",
            description: "",
            kind: { kind: "area" },
            state: "archived",
          },
        ]);
      }
      assert.equal(arguments_[0], "project");
      return reply([{ id: projectId, board: boardId, title: "preserve me" }]);
    },
  });
  assert.equal(calls.length, 3);
  assert.equal(result.boards[0]?.facts.state, "archived");
  assert.equal(result.folders[0]?.facts.title, "preserve me");
  assert.equal("envelope" in (result.workspaces[0]?.facts ?? {}), false);
});
test("empty lists and wrong CLI versions are incomplete, not an empty successful snapshot", async (): Promise<void> => {
  await assert.rejects(
    collectRouterLists({ expectedVersion: "0.1.125", read: async () => reply([]) }),
    /empty/,
  );
  await assert.rejects(
    collectRouterLists({ expectedVersion: "0.1.135", read: async () => reply([]) }),
    /version/,
  );
});
test("repeated cursor is refused rather than hiding a truncated enumeration", async (): Promise<void> => {
  await assert.rejects(
    collectRouterLists({
      expectedVersion: "0.1.125",
      read: async () =>
        reply(
          [{ workspaceId, name: "workspace", description: "", state: "active", repositories: [] }],
          "repeat",
        ),
    }),
    /repeated/,
  );
});

test("staging graph capture keeps ADR bodies and repository references, including closed Initiative facts", async (): Promise<void> => {
  const adrId = "01900000-0000-7000-8000-000000000004";
  const repository = { kind: "remote", host: "example.test", path: "synthetic/repository" };
  const result = await collectRouterLists({
    expectedVersion: "0.1.125",
    includeWorkspaceGraph: true,
    read: async (arguments_) => {
      if (arguments_[0] === "workspace")
        return reply([
          {
            workspaceId,
            name: "archived workspace",
            description: "",
            state: "archived",
            repositories: [repository],
          },
        ]);
      if (arguments_[0] === "board")
        return reply([
          {
            boardId,
            workspaceId,
            name: "closed initiative",
            description: "",
            kind: { kind: "initiative", goal: "literal goal", doneWhen: "literal completion" },
            state: "closed",
          },
        ]);
      if (arguments_[0] === "adr")
        return reply([
          {
            kind: "adr",
            record: {
              adrId,
              workspace: workspaceId,
              title: "saved choice",
              text: "literal ADR body",
              status: "accepted",
              supersedes: null,
              madeIn: null,
              repositoryFiles: [],
              revision: 2n,
              envelope: { ignoredObservationTime: "now" },
            },
          },
        ]);
      return reply([{ id: projectId, board: boardId, title: "preserve me" }]);
    },
  });
  assert.equal(result.workspaceGraphComplete, true);
  assert.equal(result.workspaceAdrs[0]?.facts.text, "literal ADR body");
  assert.equal(result.workspaceAdrs[0]?.facts.status, "accepted");
  assert.equal("envelope" in (result.workspaceAdrs[0]?.facts ?? {}), false);
  assert.deepEqual(result.workspaces[0]?.facts.repositories, [repository]);
  assert.deepEqual(result.boards[0]?.facts.kind, {
    kind: "initiative",
    goal: "literal goal",
    doneWhen: "literal completion",
  });
});

test("ADR graph collection refuses flat records and a different tagged record kind", async () => {
  const record = {
    adrId: "01900000-0000-7000-8000-000000000004",
    workspace: workspaceId,
    title: "saved choice",
    text: "literal ADR body",
    status: "accepted",
    supersedes: null,
    madeIn: null,
    repositoryFiles: [],
    revision: 2n,
  };
  for (const invalid of [record, { kind: "thread", record }])
    await assert.rejects(
      collectRouterLists({
        expectedVersion: "0.1.125",
        includeWorkspaceGraph: true,
        read: async (arguments_) => {
          if (arguments_[0] === "workspace")
            return reply([
              {
                workspaceId,
                name: "workspace",
                description: "",
                state: "active",
                repositories: [],
              },
            ]);
          if (arguments_[0] === "board")
            return reply([
              {
                boardId,
                workspaceId,
                name: "board",
                description: "",
                kind: { kind: "area" },
                state: "open",
              },
            ]);
          if (arguments_[0] === "adr") return reply([invalid]);
          return reply([{ id: projectId, board: boardId, title: "project" }]);
        },
      }),
      /adr/,
    );
});
