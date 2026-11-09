// PD7's last step, after main's central is deployed over the migrated objects:
// bring every Copy to main's receiver corpus, then confirm each one's version.
// Main reads Copy health without migrating, so until this runs a Copy whose
// corpus is older than main's reads "unreadable".
//
// 1. POST /admin/v1/copies/migrate?limit=25, paged by `after` until nextAfter
//    is null. That covers every Copy in the Fleet directory. Each result must be
//    `migrated` or `current`; anything else stops the run.
// 2. POST /admin/v1/copies/{id}/repair for each retired project (no directory
//    entry, so the page skips its Copy). It must answer 410 `retired`.
// 3. GET /admin/v1/copies/{id}/health for every project: a live Copy must
//    report the expected receiver version stored and supported; a retired Copy
//    must read `retired`, which reports no version.

export type OwnerCall = (
  method: "GET" | "POST",
  path: string,
) => Promise<{ status: number; body: unknown }>;

export interface InventoryProject {
  readonly project: string;
  readonly lifecycle: string;
}

export interface CopyVersion {
  readonly project: string;
  readonly state: "live" | "retired";
  /** The receiver version stored; null for a retired Copy, whose health reports none. */
  readonly receiverVersion: number | null;
}

export class CopyMigrationStopped extends Error {}
const stop = (reason: string): never => {
  throw new CopyMigrationStopped(reason);
};

export async function migrateCopies(
  call: OwnerCall,
  projects: readonly InventoryProject[],
  expectedVersion: number,
): Promise<{ readonly migrated: readonly unknown[]; readonly versions: readonly CopyVersion[] }> {
  const migrated: unknown[] = [];
  let after: string | null = null;
  for (;;) {
    const path = `/admin/v1/copies/migrate?limit=25${after === null ? "" : `&after=${after}`}`;
    const page = await call("POST", path);
    if (page.status !== 200) stop(`${path} answered ${page.status}: ${JSON.stringify(page.body)}`);
    const { results, nextAfter } = page.body as {
      results: { project: string; result: { kind: string } }[];
      nextAfter: string | null;
    };
    for (const item of results) {
      if (item.result.kind !== "migrated" && item.result.kind !== "current") {
        stop(`copy ${item.project} did not migrate: ${JSON.stringify(item.result)}`);
      }
      migrated.push(item);
    }
    if (nextAfter === null || results.length === 0) break;
    after = nextAfter;
  }
  for (const { project } of projects.filter((candidate) => candidate.lifecycle === "retired")) {
    const repair = await call("POST", `/admin/v1/copies/${project}/repair`);
    if (repair.status !== 410 || (repair.body as { kind?: string } | null)?.kind !== "retired") {
      stop(
        `retired copy ${project} repair answered ${repair.status}: ${JSON.stringify(repair.body)}`,
      );
    }
  }
  const versions: CopyVersion[] = [];
  for (const { project, lifecycle } of projects) {
    const health = await call("GET", `/admin/v1/copies/${project}/health`);
    if (health.status !== 200) stop(`copy ${project} health answered ${health.status}`);
    const body = health.body as {
      kind: string;
      value?: {
        bookkeeping?: {
          kind: string;
          value?: { corpus: string; stored: number; supported: number }[];
        };
      };
    };
    if (lifecycle === "retired") {
      if (body.kind !== "retired") stop(`retired copy ${project} reads ${JSON.stringify(body)}`);
      versions.push({ project, state: "retired", receiverVersion: null });
      continue;
    }
    const receiver = body.value?.bookkeeping?.value?.find(
      (entry) => entry.corpus === "fleet-sync-receiver",
    );
    if (body.kind !== "live" || receiver === undefined)
      stop(`copy ${project} reads ${JSON.stringify(body)}`);
    if (receiver?.stored !== expectedVersion || receiver.supported !== expectedVersion) {
      stop(
        `copy ${project} is at receiver ${receiver?.stored} of ${receiver?.supported}; expected ${expectedVersion}`,
      );
    }
    versions.push({ project, state: "live", receiverVersion: receiver?.stored ?? null });
  }
  return { migrated, versions };
}
