// Runs one guarded deploy (worker_deploy.ts) for real: a fresh owner-private
// scratch directory holding the copied artifact and the rendered config, the
// cf CLI run there with every output kept in private files, and the before and
// after receipts. cf uses its own owner login; token or key environments are
// refused, as fleet-ops requires.

import { execFile } from "node:child_process";
import { cp, mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import type { WorkerConfig } from "./deploy_configs.ts";
import {
  BUILD_RULES,
  deployWorker,
  type CfRunner,
  type DeployOutcome,
  type DeploySettings,
  type WorkerKind,
} from "./worker_deploy.ts";

const run = promisify(execFile);
const FORBIDDEN_AUTH = ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_API_KEY", "CF_API_TOKEN", "CF_API_KEY"];

export interface GuardedRun {
  readonly kind: WorkerKind;
  readonly settings: DeploySettings;
  readonly config: WorkerConfig;
  readonly artifact: string;
  /** Must not exist yet; created with mode 0700. */
  readonly scratch: string;
  readonly cf: string;
  readonly dryRun: boolean;
}

export async function guardedRun(options: GuardedRun): Promise<DeployOutcome> {
  for (const name of FORBIDDEN_AUTH) {
    if (process.env[name] !== undefined)
      throw new Error(`${name} is set; cf must use the owner's login`);
  }
  const scratch = resolve(options.scratch);
  await mkdir(scratch, { mode: 0o700 });
  await cp(resolve(options.artifact), join(scratch, "artifact"), {
    recursive: true,
    errorOnExist: true,
  });
  const config = `export default ${JSON.stringify(options.config, null, 2)};\n`;
  await writeFile(join(scratch, "cloudflare.config.ts"), config, { mode: 0o600 });
  await writeFile(join(scratch, "wrangler.config.ts"), BUILD_RULES[options.kind], { mode: 0o600 });
  await writeFile(join(scratch, "package.json"), '{ "type": "module" }\n', { mode: 0o600 });
  const cf: CfRunner = async (args, label) => {
    try {
      const { stdout, stderr } = await run(options.cf, [...args], {
        cwd: scratch,
        maxBuffer: 64 * 1024 * 1024,
      });
      await writeFile(join(scratch, `${label}.stdout`), stdout, { mode: 0o600, flag: "wx" });
      await writeFile(join(scratch, `${label}.stderr`), stderr, { mode: 0o600, flag: "wx" });
      return stdout;
    } catch (error) {
      const failed = error as { stdout?: string; stderr?: string };
      await writeFile(join(scratch, `${label}.stdout`), failed.stdout ?? "", {
        mode: 0o600,
        flag: "wx",
      });
      await writeFile(join(scratch, `${label}.stderr`), failed.stderr ?? "", {
        mode: 0o600,
        flag: "wx",
      });
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
