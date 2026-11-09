// The two deploy configurations PD7 needs, derived from the live target's own
// config (fleet-ops `<target>/live-target.json` `.config`, the cf CLI's
// format) so nothing else about the Worker changes:
//
// - Step 2, the migration Worker: the same name, account, domains, settings,
//   bindings and five SQLite classes. The entrypoint changes, and one text
//   binding names the Worker to itself (MIGRATION_WORKER_NAME), which its S1
//   staging interruption hook checks (src/fault_plan.ts).
// - Step 5, main's central: Fleet and ProjectCopy stay live. The three retired
//   classes get Cloudflare's `deleted` tombstone and lose their bindings, since
//   keeping them would need their code in central (`config_export_not_in_code`,
//   Durable Object class exports reference). That deletion is permanent:
//   revert works only before this deploy.

export type ExportEntry =
  | { readonly type: "durable-object"; readonly storage: "sqlite" }
  | { readonly type: "durable-object"; readonly state: "deleted" };

export interface WorkerConfig {
  readonly accountId?: string;
  readonly worker: {
    readonly name: string;
    readonly entrypoint: string;
    readonly env: Readonly<
      Record<string, { readonly type: string; readonly [key: string]: unknown }>
    >;
    readonly exports: Readonly<Record<string, ExportEntry>>;
    readonly [key: string]: unknown;
  };
}

export const LIVE_CLASSES = ["FleetObject", "ProjectCopyObject"] as const;
export const RETIRED_CLASSES = [
  "LeaseRegistryObject",
  "ConnectionObject",
  "MailboxObject",
] as const;
const ALL_CLASSES = [...LIVE_CLASSES, ...RETIRED_CLASSES];

/** Refuses a target that isn't production's five-class central. */
export function checkLiveTarget(live: WorkerConfig): void {
  const exported = Object.keys(live.worker.exports).sort();
  if (exported.join(",") !== [...ALL_CLASSES].sort().join(",")) {
    throw new Error(`target exports [${exported.join(", ")}], expected the five central classes`);
  }
  for (const name of ALL_CLASSES) {
    const entry = live.worker.exports[name];
    if (entry === undefined || !("storage" in entry) || entry.storage !== "sqlite") {
      throw new Error(`target class ${name} is not a live SQLite class`);
    }
  }
  const bound = Object.values(live.worker.env)
    .filter((binding) => binding.type === "durable-object")
    .map((binding) => String(binding.exportName))
    .sort();
  if (bound.join(",") !== [...ALL_CLASSES].sort().join(",")) {
    throw new Error(`target binds [${bound.join(", ")}], expected one binding per central class`);
  }
  for (const binding of Object.values(live.worker.env)) {
    if (binding.type === "durable-object" && binding.worker !== live.worker.name) {
      throw new Error(`binding to ${String(binding.exportName)} names another Worker`);
    }
  }
}

/** Step 2: the migration Worker, inside the existing objects. */
export function migrationWorkerConfig(live: WorkerConfig, entrypoint: string): WorkerConfig {
  checkLiveTarget(live);
  return {
    ...live,
    worker: {
      ...live.worker,
      entrypoint,
      env: { ...live.worker.env, MIGRATION_WORKER_NAME: { type: "text", value: live.worker.name } },
    },
  };
}

/** Step 5: main's central, with the retired classes deleted. */
export function mainCentralConfig(live: WorkerConfig, entrypoint: string): WorkerConfig {
  checkLiveTarget(live);
  const env = Object.fromEntries(
    Object.entries(live.worker.env).filter(
      ([, binding]) =>
        binding.type !== "durable-object" ||
        !(RETIRED_CLASSES as readonly string[]).includes(String(binding.exportName)),
    ),
  );
  const exports: Record<string, ExportEntry> = {};
  for (const name of LIVE_CLASSES) exports[name] = { type: "durable-object", storage: "sqlite" };
  for (const name of RETIRED_CLASSES) exports[name] = { type: "durable-object", state: "deleted" };
  return { ...live, worker: { ...live.worker, entrypoint, env, exports } };
}
