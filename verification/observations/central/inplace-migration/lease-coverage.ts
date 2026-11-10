/** Public Registry lifecycle determines which native-writer and content facts must exist. */
import type { Snapshot } from "./contracts.ts";

export function registryLeaseFacts(entry: Snapshot["directory"][number]): unknown {
  if (entry.leaseSide.kind !== "known") return { kind: "notYetReported" };
  const lease = entry.leaseSide.value.lease;
  return lease.kind === "held"
    ? { kind: "held", epoch: lease.value.epoch, holder: lease.value.holder }
    : lease;
}
export function registryLeaseRows(observed: Snapshot) {
  return observed.directory.map((entry) => ({
    project: entry.project,
    lease: registryLeaseFacts(entry),
  }));
}

export function checkLeaseCoverage(observed: Snapshot, gaps: string[]): void {
  const selected = new Set(observed.routerProjects);
  const writers = new Map<string, { machine: string; holder: string; lease: bigint }>();
  for (const router of observed.routers)
    for (const writer of router.writers) {
      if (!selected.has(writer.database))
        throw new Error("current Router writer is outside the declared opened cohort");
      if (writers.has(writer.database)) throw new Error("multiple current writers for one project");
      writers.set(writer.database, {
        machine: router.machineId,
        holder: writer.holder,
        lease: writer.lease,
      });
    }
  const content = observed.routers.flatMap((router) => router.content);
  const copies = new Map(observed.copies.map((copy) => [copy.project, copy.health]));
  for (const entry of observed.directory) {
    const subject = `${observed.phase}: ${entry.project}`;
    if (entry.leaseSide.kind !== "known") {
      gaps.push(`${subject} R1 Registry lease unobserved`);
      continue;
    }
    const lease = entry.leaseSide.value.lease;
    const writer = writers.get(entry.project);
    const health = copies.get(entry.project);
    const view = content.find((row) => row.project === entry.project);
    if (lease.kind === "retired") {
      if (writer !== undefined) gaps.push(`${subject} R1 non-held project has a current writer`);
      if (health?.kind !== "retired") gaps.push(`${subject} R1 retired Registry/Copy disagree`);
      continue;
    }
    const copy =
      health?.kind === "live" && health.value.copy.kind === "known"
        ? health.value.copy.value
        : undefined;
    if (copy === undefined || copy === null || copy.database !== entry.project) {
      gaps.push(`${subject} R1 Registry/Copy identity unavailable`);
      continue;
    }
    if (lease.kind === "unheld") {
      if (writer !== undefined) gaps.push(`${subject} R1 non-held project has a current writer`);
      if (copy.holder !== null) gaps.push(`${subject} R1 unheld Copy has a holder`);
      if (selected.has(entry.project) && (view === undefined || view.holderInstallation !== null))
        gaps.push(`${subject} R3 missing unheld public content view`);
    } else {
      if (
        selected.has(entry.project) &&
        (writer === undefined ||
          writer.holder !== lease.value.holder ||
          writer.machine !== lease.value.holder ||
          writer.lease !== lease.value.epoch)
      )
        gaps.push(`${subject} R1 missing matching current native writer`);
      if (copy.holder !== lease.value.holder || copy.fence !== lease.value.epoch)
        gaps.push(`${subject} R1 Registry/Copy grant differs`);
      const holderInstallation = observed.routers.find(
        (router) => router.machineId === lease.value.holder,
      )?.installationId;
      if (
        selected.has(entry.project) &&
        (view === undefined ||
          holderInstallation === undefined ||
          view.holderInstallation !== holderInstallation)
      )
        gaps.push(`${subject} R3 missing non-holder content`);
    }
  }
  for (const database of writers.keys())
    if (!observed.directory.some((entry) => entry.project === database))
      throw new Error("current writer has no directory identity");
  for (const project of selected)
    if (!observed.directory.some((entry): boolean => entry.project === project))
      gaps.push(`${observed.phase}: selected Router project has no central identity ${project}`);
}
