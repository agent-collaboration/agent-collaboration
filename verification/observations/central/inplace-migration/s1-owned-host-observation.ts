/** Explicit current owned Host selection for renewal and retirement; never a latest-PID lookup. */
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { fileSchema } from "../../../declaration.ts";
import { decodeAuthoredJson } from "../../../exact-json.ts";
import {
  readCommandLedger,
  enrollInvocationsSince,
  type CommandLedgerEntry,
} from "../../../machines/command-ledger.ts";
import {
  loadStoppedOwnedRoot,
  readStoppedReference,
  stoppedOwnedRootSchema,
} from "../../../machines/installation-stopped-root.ts";
import { fingerprint, command } from "../../../machines/owned-process.ts";
import { readPrivateText } from "./record.ts";
import { stagingRoot } from "./s1-synthetic-cli.ts";

export const ownedHostObservationSchema = z
  .object({
    kind: z.literal("currentOwnedHostObservation"),
    root: z.literal(stagingRoot),
    continuationPlan: fileSchema,
    hostWindow: fileSchema,
    host: z.object({ pid: z.number().int().positive(), commandId: z.uuidv7() }).strict(),
  })
  .strict();
const windowSchema = z
  .object({
    openedAt: z.iso.datetime(),
    originalWindow: fileSchema,
    originalBaseline: fileSchema,
    controller: stoppedOwnedRootSchema.shape.controller,
  })
  .strict();

export function requireSelectedOwnedHost(input: {
  host: z.infer<typeof ownedHostObservationSchema>["host"];
  root: string;
  executable: string;
  port: number;
  openedAt: string;
  ledger: readonly CommandLedgerEntry[];
}): { pid: number; commandId: string; startedAt: string } {
  const starts = input.ledger.filter(
    (row) => row.kind === "started" && row.commandId === input.host.commandId,
  );
  const start = starts[0];
  if (
    starts.length !== 1 ||
    start?.kind !== "started" ||
    start.mode !== "daemon" ||
    start.executable !== input.executable ||
    start.arguments[0] !== "host" ||
    start.arguments.indexOf("--router-root") < 0 ||
    start.arguments[start.arguments.indexOf("--router-root") + 1] !== input.root ||
    start.arguments[start.arguments.indexOf("--port") + 1] !== String(input.port)
  )
    throw new Error("selected Host command does not belong to the declared owned generation");
  const spawns = input.ledger.filter(
    (row) => row.kind === "spawned" && row.commandId === input.host.commandId,
  );
  if (
    spawns.length !== 1 ||
    spawns[0]?.kind !== "spawned" ||
    spawns[0].pid !== input.host.pid ||
    input.ledger.some((row) => row.kind === "ended" && row.commandId === input.host.commandId)
  )
    throw new Error("selected owned Host is not the still-running declared child");
  if (Date.parse(start.at) < Date.parse(input.openedAt))
    throw new Error("selected Host predates the declared continuation window");
  return { ...input.host, startedAt: start.at };
}
export function requireOwnedWindowLink(input: {
  window: z.infer<typeof windowSchema>;
  reference: z.infer<typeof fileSchema>;
  declaration: z.infer<typeof stoppedOwnedRootSchema>;
}): void {
  const { window, declaration } = input;
  if (
    input.reference.path !== `${declaration.evidenceDirectory}/continuation-host-window.json` ||
    !isDeepStrictEqual(window.originalWindow, declaration.original.hostWindow) ||
    !isDeepStrictEqual(window.originalBaseline, declaration.original.baseline) ||
    !isDeepStrictEqual(window.controller, declaration.controller)
  )
    throw new Error(
      "selected Host window is not linked to the original baseline and declared controller",
    );
}
export async function loadOwnedHostObservation(path: string) {
  const selection = ownedHostObservationSchema.parse(
    decodeAuthoredJson(await readPrivateText(path)),
  );
  const context = await loadStoppedOwnedRoot(
    decodeAuthoredJson(await readStoppedReference(selection.continuationPlan)),
  );
  if (context.declaration.root !== selection.root)
    throw new Error("selected Host root differs from original baseline");
  const window = windowSchema.parse(
    decodeAuthoredJson(await readStoppedReference(selection.hostWindow)),
  );
  requireOwnedWindowLink({
    window,
    reference: selection.hostWindow,
    declaration: context.declaration,
  });
  const host = requireSelectedOwnedHost({
    host: selection.host,
    root: selection.root,
    executable: context.machine.router.path,
    port: context.machine.port,
    openedAt: window.openedAt,
    ledger: await readCommandLedger(selection.root),
  });
  if (
    (await enrollInvocationsSince(selection.root, context.baseline.baselineId)) !== 0 ||
    (await fingerprint(`${selection.root}/fleet/enrollment.json`)) !==
      context.baseline.enrollmentSha256
  )
    throw new Error("original enrollment or zero-enroll marker differs for selected Host");
  return {
    selection,
    host,
    window,
    originalBaseline: context.baseline,
    selectionSha256: await fingerprint(path),
  };
}
export async function readOwnedHostProcess(
  binding: Awaited<ReturnType<typeof loadOwnedHostObservation>>,
) {
  return command("/bin/ps", ["-p", String(binding.host.pid), "-o", "pid=,lstart=,args="], {
    cwd: binding.selection.root,
  });
}
