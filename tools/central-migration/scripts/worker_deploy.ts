// One guarded deploy for the four Workers the staging and production runs put
// under one central name, in order: the staging seed (TypeScript, first upload),
// the exact old central (Rust bundle), the migration Worker (TypeScript) and
// main's central (Rust bundle, PD7 step 5 with the retired classes deleted).
//
// Every deploy renders its config from the private settings into an owner-
// private scratch directory, runs the cf CLI there, and checks cf's own
// inventory before and after: Durable Object namespaces, the Worker and the
// hostname. It refuses production's name or domain, an existing hostname at
// first upload, and any namespace change other than the one the step plans.
// A dry run renders, runs cf's own `deploy --dry-run` over the build the pinned
// cf already checked (deploy_run.ts), and prints the planned change without
// uploading.

import { mainCentralConfig, migrationWorkerConfig, type WorkerConfig } from "./deploy_configs.ts";

export type WorkerKind = "seed" | "old-central" | "migration" | "main-central";

/** Production central's identity, never a target of this tool. */
export const PRODUCTION = {
  worker: "collaboration-central",
  domain: "central.agent-collaboration.com",
} as const;

/**
 * A hostname or Worker name as Cloudflare matches it: DNS names are
 * case-insensitive and may carry a trailing dot, and Worker names are stored
 * lowercase. Every production refusal and inventory match compares these.
 */
export const canonicalName = (value: string): string =>
  value.trim().toLowerCase().replace(/\.$/, "");
const sameName = (a: string | undefined, b: string): boolean =>
  a !== undefined && canonicalName(a) === canonicalName(b);

export const CENTRAL_CLASSES = [
  "ProjectCopyObject",
  "LeaseRegistryObject",
  "FleetObject",
  "ConnectionObject",
  "MailboxObject",
] as const;
const BINDINGS: Readonly<Record<(typeof CENTRAL_CLASSES)[number], string>> = {
  ProjectCopyObject: "PROJECT_COPIES",
  LeaseRegistryObject: "LEASE_REGISTRY",
  FleetObject: "FLEET",
  ConnectionObject: "CONNECTIONS",
  MailboxObject: "MAILBOXES",
};
const RETIRED_AT_STEP_5 = ["LeaseRegistryObject", "ConnectionObject", "MailboxObject"] as const;

/** The private settings file's shape (keys only are public; values stay private). */
export interface DeploySettings {
  readonly accountId: string;
  readonly workerName: string;
  readonly hostname: string;
  readonly compatibilityDate: string;
  readonly observability?: unknown;
  readonly access: {
    readonly teamDomain: string;
    readonly ownerIdentity: string;
    readonly sessionAudience: string;
    readonly adminAudience: string;
  };
  readonly central: {
    readonly registryConfig: unknown;
    readonly deliveryConfig: unknown;
    readonly holderAnswerTimeout: string;
    readonly copyAckWait: string;
  };
}

/** Where each kind's entry module sits inside the copied artifact directory. */
export const ENTRYPOINTS: Readonly<Record<WorkerKind, string>> = {
  seed: "artifact/verification/central-seed-worker/src/worker.js",
  migration: "artifact/worker.js",
  "old-central": "artifact/index.js",
  "main-central": "artifact/index.js",
};
/** cf's build-step rules per artifact: tsc output as ES modules, or worker-build's JS with its wasm. */
export const BUILD_RULES: Readonly<Record<WorkerKind, string>> = {
  seed: 'export default { noBundle: true, rules: [{ type: "ESModule", globs: ["**/*.js"], fallthrough: false }] };\n',
  migration:
    'export default { noBundle: true, rules: [{ type: "ESModule", globs: ["**/*.js"], fallthrough: false }] };\n',
  "old-central":
    'export default { noBundle: true, rules: [{ type: "CompiledWasm", globs: ["**/*.wasm"], fallthrough: false }] };\n',
  "main-central":
    'export default { noBundle: true, rules: [{ type: "CompiledWasm", globs: ["**/*.wasm"], fallthrough: false }] };\n',
};

export class DeployRefused extends Error {}
const refuse = (reason: string): never => {
  throw new DeployRefused(reason);
};

/** Old central's five-class config at the S1 name, from the private settings. */
export function liveConfig(settings: DeploySettings, includeDomain: boolean): WorkerConfig {
  const text = (value: string) => ({ type: "text", value });
  const storage = (exportName: string) => ({
    type: "durable-object",
    worker: settings.workerName,
    exportName,
  });
  return {
    accountId: settings.accountId,
    worker: {
      name: settings.workerName,
      entrypoint: ENTRYPOINTS["old-central"],
      compatibilityDate: settings.compatibilityDate,
      workersDev: false,
      previewUrls: false,
      ...(settings.observability === undefined || settings.observability === null
        ? {}
        : { observability: settings.observability }),
      domains: includeDomain ? [settings.hostname] : [],
      env: {
        ACCESS_TEAM_DOMAIN: text(settings.access.teamDomain),
        ACCESS_SESSION_AUD: text(settings.access.sessionAudience),
        ACCESS_ADMIN_AUD: text(settings.access.adminAudience),
        ACCESS_OWNER_IDENTITY: text(settings.access.ownerIdentity),
        CENTRAL_HOLDER_ANSWER_TIMEOUT: text(settings.central.holderAnswerTimeout),
        CENTRAL_COPY_ACK_WAIT: text(settings.central.copyAckWait),
        CENTRAL_REGISTRY_CONFIG: text(JSON.stringify(settings.central.registryConfig)),
        CENTRAL_DELIVERY_CONFIG: text(JSON.stringify(settings.central.deliveryConfig)),
        ...Object.fromEntries(CENTRAL_CLASSES.map((name) => [BINDINGS[name], storage(name)])),
      },
      exports: Object.fromEntries(
        CENTRAL_CLASSES.map((name) => [name, { type: "durable-object", storage: "sqlite" }]),
      ),
    },
  };
}

/**
 * The config one central kind deploys, rendered from the same live shape. The
 * staging seed renders its own (verification/central-seed-worker), from
 * liveConfig, and passes it to the same guarded run.
 */
export function renderConfig(
  kind: Exclude<WorkerKind, "seed">,
  settings: DeploySettings,
): WorkerConfig {
  const live = liveConfig(settings, true);
  const config =
    kind === "migration"
      ? migrationWorkerConfig(live, ENTRYPOINTS.migration)
      : kind === "main-central"
        ? mainCentralConfig(live, ENTRYPOINTS["main-central"])
        : live;
  return refuseProduction(config);
}

/** Refuses a config that names production central's Worker or domain. */
export function refuseProduction(config: WorkerConfig): WorkerConfig {
  if (
    sameName(config.worker.name, PRODUCTION.worker) ||
    JSON.stringify(config).toLowerCase().includes(PRODUCTION.domain)
  ) {
    refuse("the rendered config names production central");
  }
  return config;
}

/** A Durable Object namespace as cf lists it. */
export interface Namespace {
  readonly id: string;
  readonly name: string | undefined;
  readonly script: string | undefined;
  readonly class: string | undefined;
}
export interface Inventory {
  readonly namespaces: readonly Namespace[];
  readonly workers: readonly {
    readonly id?: string;
    readonly script_name?: string;
    readonly name?: string;
  }[];
  readonly domains: readonly { readonly hostname?: string; readonly service?: string }[];
}

const ownNamespaces = (inventory: Inventory, worker: string) =>
  inventory.namespaces.filter((namespace) => sameName(namespace.script, worker));
const others = (inventory: Inventory, worker: string) =>
  inventory.namespaces
    .filter((namespace) => !sameName(namespace.script, worker))
    .map((namespace) => JSON.stringify(namespace))
    .sort();
const workerNames = (inventory: Inventory) =>
  inventory.workers.map((row) => canonicalName(row.script_name ?? row.id ?? row.name ?? ""));
const boundTo = (inventory: Inventory, hostname: string) =>
  inventory.domains.filter((domain) => sameName(domain.hostname, hostname));

/** Before uploading: the kind's preconditions on the current inventory. */
export function checkBefore(kind: WorkerKind, settings: DeploySettings, before: Inventory): void {
  const worker = settings.workerName;
  if (sameName(worker, PRODUCTION.worker) || sameName(settings.hostname, PRODUCTION.domain))
    refuse("the target is production central");
  const own = ownNamespaces(before, worker);
  const host = boundTo(before, settings.hostname);
  if (kind === "seed") {
    if (workerNames(before).includes(canonicalName(worker)))
      refuse(`Worker ${worker} already exists; the seed is the first upload`);
    if (own.length > 0) refuse(`${own.length} namespaces already belong to ${worker}`);
    if (host.length > 0) refuse(`hostname ${settings.hostname} already exists`);
    return;
  }
  if (!workerNames(before).includes(canonicalName(worker)))
    refuse(`Worker ${worker} doesn't exist yet`);
  const classes = own.map((namespace) => namespace.class).sort();
  if (classes.join(",") !== [...CENTRAL_CLASSES].sort().join(",")) {
    refuse(`${worker} holds namespaces [${classes.join(", ")}], not the five central classes`);
  }
  if (host.length !== 1 || !sameName(host[0]?.service, worker))
    refuse(`hostname ${settings.hostname} isn't bound to ${worker} alone`);
}

/** After uploading: exactly the planned namespace change, and nothing else touched. */
export function checkAfter(
  kind: WorkerKind,
  settings: DeploySettings,
  before: Inventory,
  after: Inventory,
): void {
  const worker = settings.workerName;
  if (others(before, worker).join("\n") !== others(after, worker).join("\n")) {
    refuse("another Worker's namespaces changed");
  }
  const own = ownNamespaces(after, worker);
  const byClass = new Map(own.map((namespace) => [namespace.class, namespace.id]));
  const previous = new Map(
    ownNamespaces(before, worker).map((namespace) => [namespace.class, namespace.id]),
  );
  const expected: readonly string[] =
    kind === "main-central"
      ? CENTRAL_CLASSES.filter((name) => !(RETIRED_AT_STEP_5 as readonly string[]).includes(name))
      : CENTRAL_CLASSES;
  const classes = [...byClass.keys()].sort();
  if (own.length !== byClass.size || classes.join(",") !== [...expected].sort().join(",")) {
    refuse(
      `${worker} holds namespaces [${classes.join(", ")}] after deploy; expected [${[...expected].sort().join(", ")}]`,
    );
  }
  if (kind !== "seed") {
    for (const name of expected) {
      if (byClass.get(name) !== previous.get(name)) refuse(`namespace for ${name} changed id`);
    }
  }
  const host = boundTo(after, settings.hostname);
  if (host.length !== 1 || !sameName(host[0]?.service, worker))
    refuse(`hostname ${settings.hostname} isn't bound to ${worker} after deploy`);
}

/** One cf CLI, run in the private scratch directory; returns stdout. */
export type CfRunner = (args: readonly string[], label: string) => Promise<string>;
/**
 * The two cf roles: `inventory` reads the account (Durable Object namespaces,
 * Worker scripts, Workers custom domains) and `deploy` uploads the build the
 * pinned cf checked. They are different releases (deploy_run.ts).
 */
export interface CfRunners {
  readonly inventory: CfRunner;
  readonly deploy: CfRunner;
}

/** cf answers a list either as an array or as {success, result} (fleet-ops deploy guard). */
export function rows(text: string): Record<string, unknown>[] {
  const parsed: unknown = JSON.parse(text);
  if (Array.isArray(parsed)) return parsed as Record<string, unknown>[];
  const envelope = parsed as { success?: boolean; result?: unknown };
  if (envelope.success === true && Array.isArray(envelope.result))
    return envelope.result as Record<string, unknown>[];
  return refuse("cf answered a list in an unknown shape; refusing to read it as empty");
}

async function pages(
  cf: CfRunner,
  command: readonly string[],
  label: string,
): Promise<Record<string, unknown>[]> {
  const all: Record<string, unknown>[] = [];
  for (let page = 1; page <= 1000; page += 1) {
    const found = rows(
      await cf([...command, "--page", String(page), "--per-page", "100"], `${label}-${page}`),
    );
    all.push(...found);
    if (found.length < 100) return all;
  }
  return refuse("inventory exceeded its bound without a terminal page");
}

const optionalText = (value: unknown): string | undefined =>
  typeof value === "string" ? value : undefined;
function namespaceOf(row: Record<string, unknown>): Namespace {
  if (typeof row.id !== "string") return refuse("cf listed a namespace without an id");
  return {
    id: row.id,
    name: optionalText(row.name),
    script: optionalText(row.script),
    class: optionalText(row.class),
  };
}

export async function takeInventory(
  cf: CfRunner,
  settings: DeploySettings,
  phase: string,
): Promise<Inventory> {
  return {
    namespaces: (
      await pages(cf, ["durable-objects", "namespaces", "list"], `${phase}-namespaces`)
    ).map(namespaceOf),
    workers: rows(
      await cf(["workers", "scripts", "search", "--name", settings.workerName], `${phase}-workers`),
    ).filter((row) =>
      sameName(optionalText(row.script_name ?? row.id ?? row.name), settings.workerName),
    ),
    domains: rows(
      await cf(["workers", "domains", "list", "--hostname", settings.hostname], `${phase}-domains`),
    ),
  };
}

/** The namespace diff a deploy plans, from the before inventory. */
export interface NamespacePlan {
  readonly create: readonly string[];
  readonly keep: readonly { readonly class: string; readonly id: string }[];
  readonly delete: readonly { readonly class: string; readonly id: string }[];
}

export interface DeployOutcome {
  readonly kind: WorkerKind;
  readonly dryRun: boolean;
  readonly before: Inventory;
  readonly after: Inventory | null;
  readonly planned: string;
  readonly namespaces: NamespacePlan;
}

export function planNamespaces(
  kind: WorkerKind,
  settings: DeploySettings,
  before: Inventory,
): NamespacePlan {
  const own = ownNamespaces(before, settings.workerName);
  if (kind === "seed") return { create: [...CENTRAL_CLASSES], keep: [], delete: [] };
  const retired = kind === "main-central" ? (RETIRED_AT_STEP_5 as readonly string[]) : [];
  const entry = (namespace: Namespace) => ({ class: String(namespace.class), id: namespace.id });
  return {
    create: [],
    keep: own.filter((namespace) => !retired.includes(String(namespace.class))).map(entry),
    delete: own.filter((namespace) => retired.includes(String(namespace.class))).map(entry),
  };
}

/** The planned namespace change, in words, for the dry run and the receipt. */
export function plannedChange(kind: WorkerKind): string {
  switch (kind) {
    case "seed":
      return "create the Worker with five new namespaces and its hostname";
    case "main-central":
      return "keep FleetObject and ProjectCopyObject; delete LeaseRegistryObject, ConnectionObject and MailboxObject";
    default:
      return "keep all five namespaces; change only the code";
  }
}

/** Checks, deploys (or dry-runs) and re-checks; the config files must already be in the scratch directory. */
export async function deployWorker(
  kind: WorkerKind,
  settings: DeploySettings,
  cf: CfRunners,
  options: { readonly dryRun: boolean; readonly tag: string; readonly message: string },
): Promise<DeployOutcome> {
  const before = await takeInventory(cf.inventory, settings, "before");
  checkBefore(kind, settings, before);
  const namespaces = planNamespaces(kind, settings, before);
  if (options.dryRun) {
    await cf.deploy(["deploy", "--dry-run", "--prebuilt"], "deploy-dry-run");
    return { kind, dryRun: true, before, after: null, planned: plannedChange(kind), namespaces };
  }
  await cf.deploy(
    ["deploy", "--prebuilt", "--tag", options.tag, "--message", options.message],
    "deploy",
  );
  const after = await takeInventory(cf.inventory, settings, "after");
  checkAfter(kind, settings, before, after);
  return { kind, dryRun: false, before, after, planned: plannedChange(kind), namespaces };
}
