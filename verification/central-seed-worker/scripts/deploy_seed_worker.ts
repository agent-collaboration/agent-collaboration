// The staging seed's first upload under the S1 name, through the migration
// tool's guarded deploy (tools/central-migration/scripts/deploy_run.ts): the
// Worker must not exist yet, its five namespaces and its hostname are new, and
// nothing else in the account changes.
//
//   node scripts/deploy_seed_worker.ts --settings <private settings.json>
//     --artifact <this package's build/> --scratch <new private dir> [--dry-run]

import { readFile } from "node:fs/promises";
import { PINNED_CF, guardedRun } from "../../../tools/central-migration/scripts/deploy_run.ts";
import {
  DeployRefused,
  ENTRYPOINTS,
  liveConfig,
  refuseProduction,
  type DeploySettings,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";
import { seedWorkerConfig } from "./deploy_config.ts";

const flags = process.argv.slice(2);
const option = (name: string): string | undefined => {
  const index = flags.indexOf(`--${name}`);
  return index < 0 ? undefined : flags[index + 1];
};
const settingsPath = option("settings");
const artifact = option("artifact");
const scratch = option("scratch");
if (settingsPath === undefined || artifact === undefined || scratch === undefined) {
  throw new Error(
    "usage: deploy_seed_worker.ts --settings <file> --artifact <dir> --scratch <dir> [--dry-run]",
  );
}
const settings = JSON.parse(await readFile(settingsPath, "utf8")) as DeploySettings;

try {
  const outcome = await guardedRun({
    kind: "seed",
    settings,
    config: refuseProduction(seedWorkerConfig(liveConfig(settings, true), ENTRYPOINTS.seed)),
    artifact,
    scratch,
    cf: PINNED_CF,
    inventoryCf: "cf",
    dryRun: flags.includes("--dry-run"),
  });
  console.log(
    JSON.stringify({
      kind: "seed",
      dryRun: outcome.dryRun,
      planned: outcome.planned,
      namespaces: outcome.namespaces,
      outcome: "complete",
    }),
  );
} catch (error) {
  if (!(error instanceof DeployRefused)) throw error;
  console.error(JSON.stringify({ kind: "seed", outcome: "refused", reason: error.message }));
  process.exitCode = 1;
}
