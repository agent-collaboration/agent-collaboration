import assert from "node:assert/strict";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { claimableImportedProject, verifyClaimedImport } from "./clients/import-claim.ts";
import { originalImportHeader, type CentralImportReadback } from "./clients/import-readback.ts";
import { encode } from "./exact-json.ts";
import { Receipt } from "./receipt.ts";
import { withMcpFixture } from "./test-support/mcp-fixture.ts";

const projectId = "019a0000-0000-7000-8000-000000000015";
const peerId = "019a0000-0000-7000-8000-000000000016";
const boardId = "019a0000-0000-7000-8000-000000000017";
const manifest = {
  database: projectId,
  position: { epoch: 1n, sequence: 73n },
  schema: { version: 3n, fingerprint: "synthetic" },
  tables: [{ name: "posts", columns: [{ name: "body", type: "text" }] }],
};
const project = {
  project: projectId,
  workspace: peerId,
  board: boardId,
  posts: 3n,
  feedRows: 7n,
  digest: "a".repeat(64),
  manifest,
};
const lease = { epoch: 2n, expiresAt: 123456789n, start: manifest.position };
const header = {
  digest: project.digest,
  pages: 4n,
  declaredOwner: "synthetic-owner",
  provenance: { installation: "synthetic", machine: "synthetic" },
  manifest,
  summary: {
    kind: "projectSummary",
    value: { status: null, latestRecordAt: { kind: "legacyUnknown" } },
  },
};
function readback(
  copy: unknown = { holder: peerId, fence: 2n, acked: manifest.position },
): CentralImportReadback {
  return {
    copies: [
      {
        database: projectId,
        record: [encode({ state: "following", copy })],
        seed: [],
        summary: [],
        posts: ["3"],
        staged: ["0"],
      },
    ],
    workflows: [
      encode({
        project: projectId,
        digest: project.digest,
        header: encode(header),
        phase: encode({ kind: "complete", position: manifest.position }),
      }),
    ],
    directory: [],
  };
}
async function withReceipt(run: (receipt: Receipt) => Promise<void>): Promise<void> {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-10-import-claim-")));
  try {
    await run(new Receipt(root));
  } finally {
    await rm(root, { recursive: true });
  }
}
test("completed import replay retains every original header field and physical table descriptor", () => {
  assert.deepEqual(originalImportHeader(readback(), projectId), header);
});
test("original header retrieval rejects an absent or repeated workflow", () => {
  const stored = readback();
  assert.throws(
    () => originalImportHeader({ ...stored, workflows: [] }, projectId),
    /absent or repeated/,
  );
  assert.throws(
    () =>
      originalImportHeader(
        { ...stored, workflows: [...stored.workflows, ...stored.workflows] },
        projectId,
      ),
    /absent or repeated/,
  );
});
test("original header retrieval rejects a workflow/header identity mismatch", () => {
  const stored = readback();
  stored.workflows = [
    encode({
      project: projectId,
      digest: project.digest,
      header: encode({ ...header, manifest: { ...manifest, database: peerId } }),
      phase: "{}",
    }),
  ];
  assert.throws(() => originalImportHeader(stored, projectId), /identities differ/);
});
test("claim checker accepts the owning peer at epoch two and the original physical prefix", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    await verifyClaimedImport({ stored: readback(), project, peerId, claimed: lease, receipt });
  });
});
test("claim checker refuses a service identity used as the Copy holder", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    await assert.rejects(
      verifyClaimedImport({
        stored: readback({ holder: boardId, fence: 2n, acked: manifest.position }),
        project,
        peerId,
        claimed: lease,
        receipt,
      }),
      /claimed machine authority/,
    );
  });
});
test("claim checker refuses a reset Copy fence or acknowledged prefix", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    for (const copy of [
      { holder: peerId, fence: 1n, acked: manifest.position },
      { holder: peerId, fence: 2n, acked: { epoch: 1n, sequence: 0n } },
    ])
      await assert.rejects(
        verifyClaimedImport({ stored: readback(copy), project, peerId, claimed: lease, receipt }),
        /claimed machine authority/,
      );
  });
});
test("claim checker refuses another native lease epoch or starting prefix", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    await assert.rejects(
      verifyClaimedImport({
        stored: readback(),
        project,
        peerId,
        claimed: { ...lease, epoch: 3n },
        receipt,
      }),
      /advances the native lease once/,
    );
    await assert.rejects(
      verifyClaimedImport({
        stored: readback(),
        project,
        peerId,
        claimed: { ...lease, start: { epoch: 1n, sequence: 0n } },
        receipt,
      }),
      /original imported prefix/,
    );
  });
});
test("claim selection skips an archived board through the public board show contract", async () => {
  const calls: unknown[] = [];
  await withMcpFixture({
    reply: (request) => {
      assert.equal(request.params.name, "board_show");
      calls.push(request.params.arguments);
      return {
        board: {
          boardId: request.params.arguments.boardId,
          state: calls.length === 1 ? "archived" : "open",
        },
      };
    },
    verify: async (client, receipt): Promise<void> => {
      const selected = { ...project, project: peerId, board: peerId };
      assert.deepEqual(
        await claimableImportedProject({ projects: [project, selected], client, receipt }),
        selected,
      );
      assert.deepEqual(calls, [{ boardId }, { boardId: peerId }]);
    },
  });
});
test("claim selection leaves an all-archived source unobserved", async () => {
  await withMcpFixture({
    reply: () => ({ board: { boardId, state: "archived" } }),
    verify: async (client, receipt): Promise<void> => {
      await assert.rejects(
        claimableImportedProject({ projects: [project], client, receipt }),
        /no open imported board/,
      );
    },
  });
});
