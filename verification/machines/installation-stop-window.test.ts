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
    updateStoppedRoot: async (): Promise<void> => {
      observations.push("updated");
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
      updateStoppedRoot: async (): Promise<void> => {
        effects.push("updated");
      },
    }),
    /owned child remains/,
  );
  assert.deepEqual(effects, []);
});

test("old checkpoint is observed before the updater, and replacement cannot start before update completion", async (): Promise<void> => {
  const updated = Promise.withResolvers<void>();
  const enteredUpdate = Promise.withResolvers<void>();
  const effects: string[] = [];
  const upgrading = observeInstallationStopWindow({
    stopOwnedChildren: async (): Promise<void> => {
      effects.push("stopped");
    },
    observeStoppedOld: async (): Promise<void> => {
      effects.push("old-checkpoint");
    },
    updateStoppedRoot: async (): Promise<void> => {
      effects.push("updater");
      enteredUpdate.resolve();
      await updated.promise;
      effects.push("updated");
    },
  }).then((): void => {
    effects.push("replacement");
  });
  // One turn lets the old implementation finish; it must not start a replacement without update.
  await Promise.race([enteredUpdate.promise, upgrading]);
  assert.deepEqual(effects, ["stopped", "old-checkpoint", "updater"]);
  updated.resolve();
  await upgrading;
  assert.deepEqual(effects, ["stopped", "old-checkpoint", "updater", "updated", "replacement"]);
});

test("a failed required update prevents replacement startup", async (): Promise<void> => {
  const effects: string[] = [];
  await assert.rejects(
    observeInstallationStopWindow({
      stopOwnedChildren: async (): Promise<void> => {
        effects.push("stopped");
      },
      updateStoppedRoot: async (): Promise<void> => {
        effects.push("updater");
        throw new Error("required update failed");
      },
    }).then((): void => {
      effects.push("replacement");
    }),
    /required update failed/,
  );
  assert.deepEqual(effects, ["stopped", "updater"]);
});
