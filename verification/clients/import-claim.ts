import { z } from "zod";
import { boardIdentity } from "./board-requests.ts";
import type { UploadedUpgrade } from "./board-upgrade.ts";
import {
  originalImportHeader,
  readCentralImport,
  type CentralImportReadback,
} from "./import-readback.ts";
import { claimProject, type ClaimedProject } from "./project-claim.ts";
import type { RouterMcp } from "./router-mcp.ts";
import { decode } from "../exact-json.ts";
import type { Installation } from "../machines/installation.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";
import type { TokioCentral } from "../targets/tokio-central.ts";

type ImportedProject = UploadedUpgrade["report"]["projects"][number];
const boardSchema = z.object({
  board: z.object({
    boardId: z.uuid(),
    state: z.enum(["open", "closed", "archiving", "archived"]),
  }),
});
const copySchema = z.object({
  state: z.literal("following"),
  copy: z.object({ holder: z.uuid(), fence: z.bigint().positive(), acked: z.unknown() }),
});
export async function claimableImportedProject(input: {
  projects: readonly ImportedProject[];
  client: RouterMcp;
  receipt: Receipt;
}): Promise<ImportedProject> {
  for (const project of input.projects) {
    const { board } = boardSchema.parse(
      await input.client.call("board_show", { boardId: project.board }),
    );
    await input.receipt.equal(
      "claim selection names the imported board",
      board.boardId,
      project.board,
    );
    if (board.state === "open") return project;
  }
  throw new ObservationOpen("no open imported board is available for the claim observation");
}
export async function verifyClaimedImport(input: {
  stored: CentralImportReadback;
  project: ImportedProject;
  peerId: string;
  claimed: ClaimedProject;
  receipt: Receipt;
}): Promise<void> {
  const copies = input.stored.copies.filter((copy) => copy.database === input.project.project);
  if (
    copies.length !== 1 ||
    copies[0] === undefined ||
    copies[0].record.length !== 1 ||
    copies[0].record[0] === undefined
  )
    throw new Error("claimed owning Copy absent or repeated");
  const copy = copySchema.parse(decode(copies[0].record[0])).copy;
  await input.receipt.equal(
    "first imported claim advances the native lease once",
    input.claimed.epoch,
    2n,
  );
  await input.receipt.equal(
    "claim starts at the original imported prefix",
    input.claimed.start,
    input.project.manifest.position,
  );
  await input.receipt.equal("owning Copy retains the claimed machine authority and prefix", copy, {
    holder: input.peerId,
    fence: input.claimed.epoch,
    acked: input.claimed.start,
  });
}
export async function verifyMacImportClaim(input: {
  installation: Installation;
  central: TokioCentral;
  projects: readonly ImportedProject[];
  receipt: Receipt;
  deadline: string;
}): Promise<ImportedProject> {
  if (input.installation.remote !== undefined) throw new Error("real import claim is Mac-only");
  const { central, installation, receipt, deadline } = input;
  const project = await claimableImportedProject({
    projects: input.projects,
    client: installation.mcp,
    receipt,
  });
  const requestId = boardIdentity();
  const request = { client: installation.mcp, projectId: project.project, requestId, deadline };
  const claimed = await claimProject(request);
  const identity = await installation.identity();
  const before = await readCentralImport({ central, receipt, phase: "claimed" });
  await verifyClaimedImport({ stored: before, project, peerId: identity.peerId, claimed, receipt });
  const header = originalImportHeader(before, project.project);
  await receipt.equal(
    "completed claimed import replay is alreadyImported",
    await central.owner.call(`/admin/v1/import/projects/${project.project}`, {
      kind: "begin",
      value: { header },
    }),
    { kind: "alreadyImported" },
  );
  const after = await readCentralImport({ central, receipt, phase: "claimed-replayed" });
  await receipt.equal(
    "claimed import replay neither reseeds nor regrants owning state",
    after,
    before,
  );
  await receipt.equal(
    "same claim attempt replays the exact installed lease",
    await claimProject(request),
    claimed,
  );
  await receipt.record("claimedImportReplay", {
    project: project.project,
    requestId,
    epoch: claimed.epoch,
    owningStateUnchanged: true,
  });
  return project;
}
