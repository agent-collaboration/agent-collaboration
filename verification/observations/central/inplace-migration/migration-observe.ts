/** CLI has only read-only capture and offline comparison; no migration/deploy command. */
import { encode } from "../../../exact-json.ts";
import { snapshot } from "./contracts.ts";
import { compareSnapshots } from "./compare.ts";
import { readPrivateJson, recordSnapshot, verifyObservationSources } from "./record.ts";

export async function main(arguments_: readonly string[]): Promise<number> {
  const [command, first, second] = arguments_;
  if (command === "--help" && arguments_.length === 1) {
    process.stdout.write(
      "Usage: node verification/observations/central/inplace-migration/migration-observe.ts record PLAN.private.json\n       node verification/observations/central/inplace-migration/migration-observe.ts compare BEFORE.private.json AFTER.private.json\nRecord uses only existing copy GET routes. Router observations are separate independent readbacks, never migration reports. Missing coverage returns incomplete; complete S1 also requires S7 and non-holder content. No staging writes, copies, deploy, enrollment or restart.\n",
    );
    return 0;
  }
  if (command === "record" && arguments_.length === 2 && first !== undefined) {
    const output = await recordSnapshot(await readPrivateJson(first));
    process.stdout.write(`${encode({ kind: "recorded", output, runtimeQualified: false })}\n`);
    return 0;
  }
  if (
    command === "compare" &&
    arguments_.length === 3 &&
    first !== undefined &&
    second !== undefined
  ) {
    const before = snapshot.parse(await readPrivateJson(first)),
      after = snapshot.parse(await readPrivateJson(second));
    await verifyObservationSources(before);
    await verifyObservationSources(after);
    const report = compareSnapshots(before, after);
    process.stdout.write(`${encode(report)}\n`);
    return report.status === "preserved" ? 0 : report.status === "changed" ? 1 : 2;
  }
  throw new Error("invalid arguments; use --help");
}
if (import.meta.main) {
  void main(process.argv.slice(2))
    .then((exit): void => {
      process.exitCode = exit;
    })
    .catch((): void => {
      // Do not print credential-bearing input, HTTP bodies or raw schema errors.
      process.stderr.write(
        `${encode({ kind: "observationFailed", detail: "capture/comparison refused; inspect private inputs and declared coverage" })}\n`,
      );
      process.exitCode = 3;
    });
}
