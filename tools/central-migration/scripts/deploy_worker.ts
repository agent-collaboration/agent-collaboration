// The guarded deploy of a central Worker under the settings' name:
//
//   node scripts/deploy_worker.ts <old-central|migration|main-central> --settings <private settings.json>
//     --artifact <build dir> --scratch <new private dir> [--dry-run]
//
// old-central and main-central take worker-build's build/ (index.js, index_bg.wasm);
// migration takes this package's build/. cf is always this package's pinned
// devDependency (deploy_run.ts), never one from PATH. The staging seed
// deploys through verification/central-seed-worker/scripts/deploy_seed_worker.ts.

import { readFile } from "node:fs/promises";
import { PINNED_CF, guardedRun } from "./deploy_run.ts";
import { DeployRefused, renderConfig, type DeploySettings } from "./worker_deploy.ts";

const [kind, ...flags] = process.argv.slice(2);
const option = (name: string): string | undefined => {
  const index = flags.indexOf(`--${name}`);
  return index < 0 ? undefined : flags[index + 1];
};
if (kind !== "old-central" && kind !== "migration" && kind !== "main-central") {
  throw new Error(
    "usage: deploy_worker.ts <old-central|migration|main-central> --settings <file> --artifact <dir> --scratch <dir> [--dry-run]",
  );
}
const settingsPath = option("settings");
const artifact = option("artifact");
const scratch = option("scratch");
if (settingsPath === undefined || artifact === undefined || scratch === undefined)
  throw new Error("--settings, --artifact and --scratch are required");
const settings = JSON.parse(await readFile(settingsPath, "utf8")) as DeploySettings;

try {
  const outcome = await guardedRun({
    kind,
    settings,
    config: renderConfig(kind, settings),
    artifact,
    scratch,
    cf: PINNED_CF,
    inventoryCf: "cf",
    dryRun: flags.includes("--dry-run"),
  });
  console.log(
    JSON.stringify({
      kind,
      dryRun: outcome.dryRun,
      planned: outcome.planned,
      namespaces: outcome.namespaces,
      outcome: "complete",
    }),
  );
} catch (error) {
  if (!(error instanceof DeployRefused)) throw error;
  console.error(JSON.stringify({ kind, outcome: "refused", reason: error.message }));
  process.exitCode = 1;
}
