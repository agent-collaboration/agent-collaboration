// Runs PD7's post-deploy Copy migration (copy_migration.ts) against a deployed
// central. The projects come from the migration Worker's inventory (its JSON
// answer, saved before migrate); the owner's Access token comes from the
// environment and is never printed.
//
//   CENTRAL_OWNER_ACCESS_TOKEN=… node scripts/run_copy_migration.ts \
//     <central origin> <inventory.json> --receiver-version 4

import { readFile } from "node:fs/promises";
import { CopyMigrationStopped, migrateCopies, type InventoryProject } from "./copy_migration.ts";

const [origin, inventoryPath, flag, versionText] = process.argv.slice(2);
if (
  origin === undefined ||
  inventoryPath === undefined ||
  flag !== "--receiver-version" ||
  versionText === undefined
) {
  throw new Error(
    "usage: run_copy_migration.ts <central origin> <inventory.json> --receiver-version <n>",
  );
}
const token = process.env.CENTRAL_OWNER_ACCESS_TOKEN;
if (token === undefined || token === "") throw new Error("CENTRAL_OWNER_ACCESS_TOKEN is not set");
if (new URL(origin).protocol !== "https:") throw new Error("the central origin must be https");
const inventory = JSON.parse(await readFile(inventoryPath, "utf8")) as {
  projects: InventoryProject[];
};

try {
  const result = await migrateCopies(
    async (method, path) => {
      const response = await fetch(new URL(path, origin), {
        method,
        headers: { "cf-access-token": token },
      });
      const text = await response.text();
      return { status: response.status, body: text === "" ? null : (JSON.parse(text) as unknown) };
    },
    inventory.projects,
    Number(versionText),
  );
  for (const version of result.versions) console.log(JSON.stringify(version));
  console.log(
    JSON.stringify({
      copies: result.versions.length,
      migratedCopies: result.migrated.length,
      outcome: "complete",
    }),
  );
} catch (error) {
  if (!(error instanceof CopyMigrationStopped)) throw error;
  console.error(JSON.stringify({ outcome: "stopped", reason: error.message }));
  process.exitCode = 1;
}
