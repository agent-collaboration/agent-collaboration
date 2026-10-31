import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { readFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { test } from "node:test";
import { z } from "zod";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { readObservedJson, processIdentitySchema } from "./evidence-models.ts";
import { lookupProcessIdentity, sameIdentity } from "./process-ownership.ts";
import { waitForProcess } from "./runtime-plan.ts";

const ownedIdentitySchema = processIdentitySchema.omit({ parent: true });
const readySchema = z.object({
  identities: z.array(ownedIdentitySchema),
  profileBefore: z.string(),
});

async function awaitReady(
  path: string,
  driver: ChildProcess,
): Promise<z.infer<typeof readySchema>> {
  const deadline = performance.now() + 10_000;
  for (;;) {
    if (existsSync(path)) {
      try {
        return readySchema.parse(readObservedJson(readFileSync(path)));
      } catch {
        // The marker is ready only once the synchronous write is complete.
      }
    }
    assert.equal(driver.exitCode, null, "driver exited before readiness");
    assert.equal(driver.signalCode, null, "driver was signalled before readiness");
    assert.ok(performance.now() < deadline, "signal fixture readiness deadline");
    await setTimeout(20);
  }
}

test(
  "SIGINT awaits real owned Host/Router settlement and preserves disposable profile bytes",
  { timeout: 30_000 },
  async (): Promise<void> => {
    const root = createFixtureDirectory("lifecycle-sigint");
    const driver = spawn(
      process.execPath,
      [new URL("./lifecycle-signal-fixture.ts", import.meta.url).pathname, root],
      { stdio: "ignore", detached: true },
    );
    const ready = join(root, "evidence/signal-ready.json");
    let identities: z.infer<typeof ownedIdentitySchema>[] = [];
    try {
      const before = await awaitReady(ready, driver);
      identities = before.identities;
      assert.equal(identities.length, 2);
      const original = Buffer.from(before.profileBefore, "base64");
      for (const identity of identities)
        assert.ok(sameIdentity(lookupProcessIdentity(identity.pid), identity));

      assert.equal(driver.kill("SIGINT"), true);
      await waitForProcess(driver, 10_000);

      assert.equal(driver.signalCode, null, "SIGINT must unwind instead of terminating directly");
      assert.equal(driver.exitCode, 130);
      const settlement = readObservedJson(
        readFileSync(join(root, "evidence/lifecycle-settlement.json")),
      );
      assert.equal(
        readObservedJson(readFileSync(join(root, "evidence/signal-owner-settlement.json")))
          .childrenReapedBeforeFixtureCleanup,
        true,
      );
      assert.equal(settlement.codexHomeUnchangedExactly, true);
      assert.deepEqual(readFileSync(join(root, "debug-profile")), original);
      for (const identity of identities) assert.equal(lookupProcessIdentity(identity.pid), null);
      assert.equal(
        readObservedJson(readFileSync(join(root, "driver-finished.json"))).failure,
        "KeyboardInterrupt",
      );
    } finally {
      if (identities.length === 0 && existsSync(ready))
        identities = readySchema.parse(readObservedJson(readFileSync(ready))).identities;
      if (driver.exitCode === null && driver.signalCode === null) {
        driver.kill("SIGKILL");
        await waitForProcess(driver, 5000);
      }
      // Negative-control runs can leave only these exact recorded fixture children.
      for (const identity of identities)
        if (sameIdentity(lookupProcessIdentity(identity.pid), identity))
          process.kill(identity.pid, "SIGTERM");
      rmSync(root, { recursive: true, force: true });
    }
  },
);
