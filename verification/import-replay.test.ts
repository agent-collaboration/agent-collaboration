import assert from "node:assert/strict";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { verifyImportReplay } from "./clients/import-replay.ts";
import type { UploadedUpgrade } from "./clients/board-upgrade.ts";
import { Receipt } from "./receipt.ts";

const project = "019a0000-0000-7000-8000-000000000015";
function uploaded(input: { replay: boolean; physicalSequence?: bigint }): UploadedUpgrade {
  const position = { epoch: 1n, sequence: input.physicalSequence ?? 1n };
  return {
    kind: "uploaded",
    report: {
      owner: "synthetic-owner",
      fleet: { provenance: { installation: "synthetic", machine: "synthetic" } },
      fleetDigest: "a".repeat(64),
      collisions: [],
      refused: [],
      skipped: [],
      projects: [
        {
          project,
          workspace: "019a0000-0000-7000-8000-000000000001",
          board: "019a0000-0000-7000-8000-00000000000b",
          posts: 1n,
          feedRows: 2n,
          digest: "b".repeat(64),
          manifest: { database: project, position, schema: { version: 1n }, tables: [] },
        },
      ],
    },
    publication: {
      fleet: input.replay
        ? { kind: "alreadyImported" }
        : { kind: "imported", counts: { workspaces: 1n, boards: 1n, directoryEntries: 1n } },
      projects: [
        {
          project,
          answer: input.replay
            ? { kind: "alreadyImported" }
            : { kind: "imported", value: { head: position } },
        },
      ],
    },
  };
}
async function withReceipt(run: (receipt: Receipt) => Promise<void>): Promise<void> {
  const root = await realpath(await mkdtemp(join(tmpdir(), "2026-10-import-replay-")));
  try {
    await run(new Receipt(root));
  } finally {
    await rm(root, { recursive: true });
  }
}
test("identical semantic import accepts alreadyImported despite a fresh physical manifest", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    await verifyImportReplay({
      initial: uploaded({ replay: false }),
      replayed: uploaded({ replay: true, physicalSequence: 3n }),
      receipt,
    });
  });
});
test("rerun checker rejects another project publication instead of alreadyImported", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    const replayed = uploaded({ replay: true });
    replayed.publication = {
      fleet: { kind: "alreadyImported" },
      projects: [
        { project, answer: { kind: "imported", value: { head: { epoch: 1n, sequence: 1n } } } },
      ],
    };
    await assert.rejects(
      verifyImportReplay({ initial: uploaded({ replay: false }), replayed, receipt }),
      /alreadyImported/,
    );
  });
});
test("rerun checker rejects a missing project acknowledgement", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    const replayed = uploaded({ replay: true });
    replayed.publication = { fleet: { kind: "alreadyImported" }, projects: [] };
    await assert.rejects(
      verifyImportReplay({ initial: uploaded({ replay: false }), replayed, receipt }),
      /project inventory/,
    );
  });
});
test("rerun checker rejects a changed Fleet digest", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    const replayed = uploaded({ replay: true });
    replayed.report.fleetDigest = "c".repeat(64);
    await assert.rejects(
      verifyImportReplay({ initial: uploaded({ replay: false }), replayed, receipt }),
      /Fleet digest/,
    );
  });
});
test("rerun checker rejects changed project content under its original identity", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    const replayed = uploaded({ replay: true });
    const first = replayed.report.projects[0];
    if (first === undefined) throw new Error("fixture project absent");
    first.digest = "d".repeat(64);
    await assert.rejects(
      verifyImportReplay({ initial: uploaded({ replay: false }), replayed, receipt }),
      /semantic project inventory/,
    );
  });
});
test("rerun checker rejects republished Fleet metadata", async () => {
  await withReceipt(async (receipt): Promise<void> => {
    const replayed = uploaded({ replay: true });
    replayed.publication = {
      fleet: { kind: "imported", counts: { workspaces: 1n, boards: 1n, directoryEntries: 1n } },
      projects: [{ project, answer: { kind: "alreadyImported" } }],
    };
    await assert.rejects(
      verifyImportReplay({ initial: uploaded({ replay: false }), replayed, receipt }),
      /Fleet alreadyImported/,
    );
  });
});
