import assert from "node:assert/strict";
import { test } from "node:test";
import { observeInstallationStopWindow } from "./installation-stop-window.ts";

test("upgrade observation waits for old-child settlement and aborts replacement on observation failure", async (): Promise<void> => {
  const stopRequested = Promise.withResolvers<void>();
  const oldChildrenSettled = Promise.withResolvers<void>();
  const observations: string[] = [];
  const upgrade = observeInstallationStopWindow({
    stopOwnedChildren: async (): Promise<void> => {
      stopRequested.resolve();
      await oldChildrenSettled.promise;
      observations.push("stopped");
    },
    observeStoppedOld: async (): Promise<void> => {
      assert.deepEqual(observations, ["stopped"]);
      observations.push("observed");
      throw new Error("checkpoint unavailable");
    },
  }).then((): void => {
    observations.push("replacement-started");
  });
  await stopRequested.promise;
  assert.deepEqual(observations, []);
  oldChildrenSettled.resolve();
  await assert.rejects(upgrade, /checkpoint unavailable/);
  assert.deepEqual(observations, ["stopped", "observed"]);
});
test("stop failure prevents the checkpoint observer and replacement", async (): Promise<void> => {
  const effects: string[] = [];
  await assert.rejects(
    observeInstallationStopWindow({
      stopOwnedChildren: async (): Promise<void> => {
        throw new Error("owned child remains");
      },
      observeStoppedOld: async (): Promise<void> => {
        effects.push("opened-cache");
      },
    }),
    /owned child remains/,
  );
  assert.deepEqual(effects, []);
});
