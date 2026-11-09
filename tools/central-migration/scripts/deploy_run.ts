// Runs one guarded deploy (worker_deploy.ts) for real: a fresh owner-private
// scratch directory holding the copied artifact and the rendered config, the
// cf CLI run there with every output kept in private files, and the before and
// after receipts. cf uses its own owner login; token or key environments are
// refused, as fleet-ops requires.
//
// The deploy is declared against one cf: this package's pinned devDependency
// (cf with the Wrangler build step it delegates to, the versions fleet-ops'
// deploy pins), never a cf found on PATH, whose config format differs. Before
// any call that reaches Cloudflare, that cf builds the scratch offline, so its
// own config loader accepts or refuses the render; the deploy then uploads
// exactly that build output (`--prebuilt`).

import { execFile } from "node:child_process";
import { cp, mkdir, readFile, symlink, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import type { WorkerConfig } from "./deploy_configs.ts";
import {
  BUILD_RULES,
  DeployRefused,
  deployWorker,
  type CfRunner,
  type DeployOutcome,
  type DeploySettings,
  type WorkerKind,
} from "./worker_deploy.ts";

const run = promisify(execFile);
const FORBIDDEN_AUTH = ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_API_KEY", "CF_API_TOKEN", "CF_API_KEY"];

const PACKAGE_ROOT = resolve(import.meta.dirname, "..");
const TOOL_MODULES = join(PACKAGE_ROOT, "node_modules");
/** The pinned cf binary every deploy runs. */
export const PINNED_CF = join(TOOL_MODULES, ".bin", "cf");
/** Where cf writes the build it checked; `deploy --prebuilt` uploads this. */
const BUILT_WORKER = ".cloudflare/output/v0/workers/default/worker.config.json";

/** The cf and Wrangler versions this package pins, and refuses to deploy without. */
export async function pinnedTool(): Promise<{ readonly cf: string; readonly wrangler: string }> {
  const manifest = JSON.parse(await readFile(join(PACKAGE_ROOT, "package.json"), "utf8")) as {
    devDependencies: Record<string, string | undefined>;
  };
  const cf = manifest.devDependencies.cf;
  const wrangler = manifest.devDependencies.wrangler;
  if (cf === undefined || wrangler === undefined)
    throw new DeployRefused("tools/central-migration/package.json pins no cf and Wrangler");
  for (const [name, version] of [
    ["cf", cf],
    ["wrangler", wrangler],
  ] as const) {
    const installed = JSON.parse(
      await readFile(join(TOOL_MODULES, name, "package.json"), "utf8"),
    ) as { version: string };
    if (installed.version !== version) {
      throw new DeployRefused(
        `installed ${name} is ${installed.version}, not the pinned ${version}; run pnpm install --frozen-lockfile in tools/central-migration`,
      );
    }
  }
  return { cf, wrangler };
}

/** A new 0700 scratch holding the artifact, the rendered config and the pinned tool. */
export async function stageScratch(
  kind: WorkerKind,
  config: WorkerConfig,
  artifact: string,
  scratch: string,
): Promise<void> {
  const tool = await pinnedTool();
  await mkdir(scratch, { mode: 0o700 });
  await cp(resolve(artifact), join(scratch, "artifact"), { recursive: true, errorOnExist: true });
  const rendered = `export default ${JSON.stringify(config, null, 2)};\n`;
  await writeFile(join(scratch, "cloudflare.config.ts"), rendered, { mode: 0o600 });
  await writeFile(join(scratch, "wrangler.config.ts"), BUILD_RULES[kind], { mode: 0o600 });
  // cf finds its build step through this manifest; both resolve to the pinned install.
  const manifest = { type: "module", devDependencies: tool };
  await writeFile(join(scratch, "package.json"), `${JSON.stringify(manifest)}\n`, { mode: 0o600 });
  await symlink(TOOL_MODULES, join(scratch, "node_modules"));
}

/** Writes one cf call's output into the scratch, private and never overwritten. */
async function keepOutput(scratch: string, label: string, stdout: string, stderr: string) {
  await writeFile(join(scratch, `${label}.stdout`), stdout, { mode: 0o600, flag: "wx" });
  await writeFile(join(scratch, `${label}.stderr`), stderr, { mode: 0o600, flag: "wx" });
}

/**
 * The pinned cf builds the scratch offline. Its config loader is the judge of
 * the render: a refusal names only the loader's verdict lines (paths and
 * reasons, no values). Returns the Worker config cf built.
 */
export async function buildWithPinnedCf(scratch: string): Promise<Record<string, unknown>> {
  const tool = await pinnedTool();
  try {
    const { stdout, stderr } = await run(PINNED_CF, ["build", "--mode", "production"], {
      cwd: scratch,
      maxBuffer: 64 * 1024 * 1024,
    });
    await keepOutput(scratch, "build", stdout, stderr);
  } catch (error) {
    const failed = error as { stdout?: string; stderr?: string };
    await keepOutput(scratch, "build", failed.stdout ?? "", failed.stderr ?? "");
    const verdict = `${failed.stdout ?? ""}\n${failed.stderr ?? ""}`
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.startsWith("✖") || line.startsWith("→"))
      .join("; ");
    throw new DeployRefused(
      `cf ${tool.cf} refuses the rendered config: ${verdict === "" ? "see the private build logs" : verdict}`,
    );
  }
  return JSON.parse(await readFile(join(scratch, BUILT_WORKER), "utf8")) as Record<string, unknown>;
}

export interface GuardedRun {
  readonly kind: WorkerKind;
  readonly settings: DeploySettings;
  readonly config: WorkerConfig;
  readonly artifact: string;
  /** Must not exist yet; created with mode 0700. */
  readonly scratch: string;
  /** The cf that talks to Cloudflare: PINNED_CF; tests pass a stand-in. */
  readonly cf: string;
  readonly dryRun: boolean;
}

export async function guardedRun(options: GuardedRun): Promise<DeployOutcome> {
  for (const name of FORBIDDEN_AUTH) {
    if (process.env[name] !== undefined)
      throw new Error(`${name} is set; cf must use the owner's login`);
  }
  const scratch = resolve(options.scratch);
  await stageScratch(options.kind, options.config, options.artifact, scratch);
  const built = await buildWithPinnedCf(scratch);
  if (built.name !== options.config.worker.name)
    throw new DeployRefused(`cf built ${String(built.name)}, not ${options.config.worker.name}`);
  const cf: CfRunner = async (args, label) => {
    try {
      const { stdout, stderr } = await run(options.cf, [...args], {
        cwd: scratch,
        maxBuffer: 64 * 1024 * 1024,
      });
      await keepOutput(scratch, label, stdout, stderr);
      return stdout;
    } catch (error) {
      const failed = error as { stdout?: string; stderr?: string };
      await keepOutput(scratch, label, failed.stdout ?? "", failed.stderr ?? "");
      throw new Error(`cf ${label} failed; read only the private logs in ${scratch}`);
    }
  };
  const outcome = await deployWorker(options.kind, options.settings, cf, {
    dryRun: options.dryRun,
    tag: `s1-${options.kind}`,
    message: `${options.kind} deploy from tools/central-migration`,
  });
  await writeFile(join(scratch, "outcome.json"), JSON.stringify(outcome, null, 2), { mode: 0o600 });
  return outcome;
}
