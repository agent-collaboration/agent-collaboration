import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { encode } from "../../../exact-json.ts";
import { snapshot, type Snapshot, type RouterObservation } from "./contracts.ts";
import { checkLeaseCoverage, registryLeaseRows } from "./lease-coverage.ts";

export interface Difference {
  readonly rule: string;
  readonly subject: string;
  readonly before: unknown;
  readonly after: unknown;
}
export interface Comparison {
  readonly status: "changed" | "incomplete" | "preserved";
  readonly differences: readonly Difference[];
  readonly coverageGaps: readonly string[];
  readonly acceptedLimitations: readonly string[];
}
/** Sort object keys only; array order remains meaningful unless its contract says it is a set. */
export function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return z.array(z.unknown()).parse(value).map(canonical);
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(z.record(z.string(), z.unknown()).parse(value))
        .sort(([left], [right]): number => left.localeCompare(right))
        .map(([key, child]): [string, unknown] => [key, canonical(child)]),
    );
  return value;
}
function keyed<TRecord>(
  rows: readonly TRecord[],
  key: (row: TRecord) => string,
): Map<string, TRecord> {
  const found = new Map<string, TRecord>();
  for (const row of rows) {
    const id = key(row);
    if (found.has(id)) throw new Error(`duplicate observation identity: ${id}`);
    found.set(id, row);
  }
  return found;
}
function contentRows(observed: Snapshot, gaps: string[]): { id: string; digest: string }[] {
  const result = [];
  for (const router of observed.routers)
    for (const view of router.content) {
      if (view.readerInstallation !== router.installationId)
        throw new Error("content reader identity differs from capture machine");
      const records = [...keyed(view.records, (row): string => row.id).values()].sort(
        (left, right): number => left.id.localeCompare(right.id),
      );
      result.push({
        id: view.project,
        digest: createHash("sha256")
          .update(encode(canonical(records)))
          .digest("hex"),
      });
    }
  if (
    result.length === 0 &&
    observed.directory.some(
      (entry) =>
        observed.routerProjects.includes(entry.project) &&
        (entry.leaseSide.kind !== "known" || entry.leaseSide.value.lease.kind !== "retired"),
    )
  )
    gaps.push("R3: no complete non-holder public content view");
  return result;
}
function checkRenewals(router: RouterObservation, gaps: string[]): void {
  if (router.authentication.kind !== "observed") {
    gaps.push(`after: R6/S7 authentication/renewal unobserved on ${router.machineLabel}`);
    return;
  }
  const renewed = router.authentication.renewed;
  for (const writer of router.writers)
    if (!renewed.some((row): boolean => isDeepStrictEqual(row, writer)))
      gaps.push(`after: R1/S7 missing same-lease renewal ${writer.database}`);
}

function checkCoverage(observed: Snapshot, gaps: string[]): void {
  const prefix = observed.phase;
  keyed(observed.routers, (row): string => row.machineId);
  keyed(observed.routers, (row): string => row.installationId);
  for (const router of observed.routers) {
    const expectedVersion = observed.routerTarget.version;
    if (router.routerVersion !== expectedVersion)
      gaps.push(
        `${prefix}: Router version differs from ${expectedVersion} on ${router.machineLabel}`,
      );
    if (router.build.sourceHead !== observed.routerTarget.sourceHead)
      gaps.push(`${prefix}: Router source differs from declared target on ${router.machineLabel}`);
    if (!router.workspaceGraphComplete)
      gaps.push(
        `${prefix}: R5 workspace ADR/repository graph unobserved on ${router.machineLabel}`,
      );
    const listed = new Set(router.folders.map((folder): string => folder.id));
    for (const project of observed.directory)
      if (!listed.has(project.project))
        gaps.push(`${prefix}: R5 Router list missing ${project.project} on ${router.machineLabel}`);
  }
  const directory = keyed(observed.directory, (row): string => row.project);
  const copies = keyed(observed.copies, (row): string => row.project);
  for (const id of directory.keys())
    if (!copies.has(id)) gaps.push(`${prefix}: missing health ${id}`);
  for (const id of copies.keys())
    if (!directory.has(id)) throw new Error("health without a directory identity");
  for (const row of copies.values())
    if (
      row.health.kind === "live" &&
      (row.health.value.copy.kind !== "known" ||
        row.health.value.copy.value === null ||
        row.health.value.stage.kind !== "known" ||
        row.health.value.stage.value === null ||
        row.health.value.projectFingerprint.kind !== "known")
    )
      gaps.push(`${prefix}: unreadable/uninitialized copy ${row.project}`);
  const routers = keyed(observed.routers, (row): string => row.machineLabel);
  for (const label of ["sunbook", "sunclaw"])
    if (!routers.has(label)) gaps.push(`${prefix}: missing ${label} Router observation`);
  checkLeaseCoverage(observed, gaps);
  for (const router of routers.values()) {
    if (
      !router.listsComplete ||
      router.workspaces.length === 0 ||
      router.boards.length === 0 ||
      router.folders.length === 0
    )
      gaps.push(`${prefix}: R5 incomplete/empty lists on ${router.machineLabel}`);
    if (!router.writerReadComplete)
      gaps.push(`${prefix}: R1 incomplete retained writer read on ${router.machineLabel}`);
    const sourceMethods = new Set(router.sources.map((source): string => source.method));
    if (
      !sourceMethods.has("routerPublicRead") ||
      (router.writers.length > 0 && !sourceMethods.has("retainedWriterRead"))
    )
      gaps.push(
        `${prefix}: independent Router source provenance missing on ${router.machineLabel}`,
      );
    if (observed.phase === "after") checkRenewals(router, gaps);
  }
}
function compareRows<TRecord>(
  differences: Difference[],
  rule: string,
  left: readonly TRecord[],
  right: readonly TRecord[],
  key: (row: TRecord) => string,
): void {
  const old = keyed(left, key),
    next = keyed(right, key);
  for (const id of new Set([...old.keys(), ...next.keys()]))
    if (!isDeepStrictEqual(canonical(old.get(id)), canonical(next.get(id))))
      differences.push({
        rule,
        subject: id,
        before: old.get(id) ?? null,
        after: next.get(id) ?? null,
      });
}

function compareDirectoryAndLeases(
  before: Snapshot,
  after: Snapshot,
  differences: Difference[],
): void {
  const directoryFacts = (observed: Snapshot): unknown[] =>
    observed.directory.map(({ project, name, workspace, board, boardArchive }): unknown => ({
      project,
      name,
      workspace,
      board,
      boardArchive,
    }));
  compareRows(
    differences,
    "R5 directory",
    directoryFacts(before),
    directoryFacts(after),
    (row): string => z.object({ project: z.string() }).parse(row).project,
  );
  compareRows(
    differences,
    "R1 Registry lease",
    registryLeaseRows(before),
    registryLeaseRows(after),
    (row): string => row.project,
  );
}

export function compareSnapshots(beforeRaw: unknown, afterRaw: unknown): Comparison {
  const before = snapshot.parse(beforeRaw),
    after = snapshot.parse(afterRaw);
  if (
    before.phase !== "before" ||
    after.phase !== "after" ||
    before.central.origin !== after.central.origin ||
    !isDeepStrictEqual([...before.routerProjects].sort(), [...after.routerProjects].sort()) ||
    before.runId === after.runId
  )
    throw new Error(
      "explicit distinct before/after runs of the same central and declared Router cohort required",
    );
  const differences: Difference[] = [],
    gaps: string[] = [];
  checkCoverage(before, gaps);
  checkCoverage(after, gaps);
  compareDirectoryAndLeases(before, after, differences);
  const protectedCopies = (observed: Snapshot): { id: string; health: unknown }[] =>
    observed.copies.map((row) => ({
      id: row.project,
      health:
        row.health.kind === "retired"
          ? row.health
          : {
              copy: row.health.value.copy,
              stage: row.health.value.stage,
              projectFingerprint: row.health.value.projectFingerprint,
            },
    }));
  compareRows(
    differences,
    "R3 copy",
    protectedCopies(before),
    protectedCopies(after),
    (row): string => row.id,
  );
  const routersByIdentity = (
    observed: Snapshot,
  ): { id: string; machineId: string; installationId: string }[] =>
    observed.routers.map((router) => ({
      id: router.machineLabel,
      machineId: router.machineId,
      installationId: router.installationId,
    }));
  compareRows(
    differences,
    "R6 machine",
    routersByIdentity(before),
    routersByIdentity(after),
    (row): string => row.id,
  );
  function records(
    observed: Snapshot,
    kind: "workspaces" | "boards" | "folders" | "workspaceAdrs" | "writers",
  ): { id: string; facts: unknown }[] {
    return observed.routers.flatMap((router): { id: string; facts: unknown }[] =>
      router[kind].map((row) => ({
        id: `${router.machineLabel}:${"database" in row ? row.database : row.id}`,
        facts: row,
      })),
    );
  }
  for (const kind of ["workspaces", "boards", "folders", "workspaceAdrs", "writers"] as const)
    compareRows(
      differences,
      kind === "writers" ? "R1 writer" : `R5 ${kind}`,
      records(before, kind),
      records(after, kind),
      (row): string => row.id,
    );
  compareRows(
    differences,
    "R3 content",
    contentRows(before, gaps),
    contentRows(after, gaps),
    (row): string => row.id,
  );
  return {
    status: differences.length > 0 ? "changed" : gaps.length > 0 ? "incomplete" : "preserved",
    differences,
    coverageGaps: [...new Set(gaps)].sort(),
    acceptedLimitations: [...new Set([...before.limitations, ...after.limitations])],
  };
}
export function parseRouterObservation(value: unknown): RouterObservation {
  return snapshot.shape.routers.element.parse(value);
}
