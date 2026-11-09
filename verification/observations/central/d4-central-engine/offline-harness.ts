/** Launch only an explicitly declared local immutable target, without inference or production access. */
import { command } from "../../../machines/owned-process.ts";
import { lstat, realpath } from "node:fs/promises";
import { Receipt } from "../../../receipt.ts";
import { runD1Harness } from "../d1-harness.ts";
import { d1PlanForOfflineCustody, type D4OfflineHarnessPlan } from "./offline-harness-plan.ts";
import { qualifyOfflineTarget } from "./offline-source.ts";
import { OfflineNativeJourney } from "./offline-native-journey.ts";

export async function runOfflineCustody(plan: D4OfflineHarnessPlan): Promise<void> {
  if (Date.parse(plan.deadline) <= Date.now()) throw new Error("declared custody deadline expired");
  if ((await realpath(plan.repository)) !== process.cwd())
    throw new Error("launch D4 from the declared compiled source root");
  // An existing directory may contain another run's immutable evidence.
  const previous = await lstat(plan.evidenceDirectory).catch((error: NodeJS.ErrnoException) => {
    if (error.code !== "ENOENT") throw error;
    return undefined;
  });
  if (previous !== undefined) throw new Error("D4 evidence directory already exists");
  const before = await qualifyOfflineTarget(plan);
  // This is the already-qualified signed upstream CLI, without a native server or home access.
  const version = await command(plan.native.path, ["--version"], {
    environment: { PATH: "/usr/bin:/bin:/usr/sbin:/sbin" },
    timeoutSeconds: 10,
  });
  if (
    version.exit !== 0 ||
    version.stdout.trim() !== plan.native.version ||
    version.stderr.trim() !== ""
  )
    throw new Error("actual native version differs from the qualified persistence route");
  const receipt = new Receipt(plan.evidenceDirectory);
  let failure: unknown;
  try {
    await runD1Harness(d1PlanForOfflineCustody(plan), new OfflineNativeJourney(plan));
  } catch (error: unknown) {
    failure = error;
  }
  const created = await lstat(plan.evidenceDirectory).catch(() => undefined);
  if (created === undefined) throw failure ?? new Error("D1 failed to create its owned evidence");
  // The D1 owner created the evidence directory before launching anything. Do not overwrite its result.
  await receipt.save("d4-inputs-before.json", { ...before, nativeVersion: version.stdout.trim() });
  try {
    await receipt.save("d4-inputs-after.json", await qualifyOfflineTarget(plan));
  } catch (error: unknown) {
    await receipt.save("d4-input-recheck-failure.json", { message: String(error) });
    failure ??= error;
  }
  await receipt.save("d4-offline-result.json", {
    journey: "J6 ten-item custody on local Tokio",
    passed: failure === undefined,
    runtimeQualified: false,
    sourceHead: plan.sourceHead,
    apparatusHead: plan.apparatusHead,
    limits: [
      "build provenance requires the retained build receipt",
      "J6b, workerd, rebuild matrices and D5 agent delivery remain unobserved",
    ],
  });
  if (failure !== undefined) throw failure;
}
