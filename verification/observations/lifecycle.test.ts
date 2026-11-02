import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { objectSchema } from "../exact-json.ts";
import {
  readObservedJson,
  processIdentitySchema,
  type ProcessIdentity,
} from "./evidence-models.ts";
import { readCarrierCorrelation } from "./native-correlation.ts";
import { lookupProcessIdentity, sameIdentity } from "./process-ownership.ts";
import {
  identityRecord,
  observeChild,
  observeOwned,
  processOperations,
  stopOwned,
} from "./runtime-processes.ts";
import { saveRecord, waitForProcess } from "./runtime-plan.ts";
import { settleInventory } from "./process-inventory.ts";
import { LifecycleFixture, harmlessChild, releaseHarmlessChild } from "./lifecycle-fixture.ts";

for (const scenario of [
  "stop",
  "reopen",
  "profile-before-host",
  "profile-after-host",
  "config-before-host",
  "config-after-host",
  "inspection-expired",
  "reopen-expired",
  "deadline",
  "early-exit",
  "inventory-timeout",
  "external-desktop-after-host",
]) {
  test(
    `lifecycle preserves original scoped outcome: ${scenario}`,
    { timeout: 15_000 },
    async (): Promise<void> => {
      const root = createFixtureDirectory("lifecycle");
      try {
        const fixture = new LifecycleFixture(root, scenario);
        const error = await fixture.drive();
        const settlement = readObservedJson(
          readFileSync(join(fixture.plan.evidence, "lifecycle-settlement.json")),
        );
        assert.equal(
          fixture.children.every(
            ({ child }) => child.exitCode !== null || child.signalCode !== null,
          ),
          true,
        );
        if (
          scenario === "stop" ||
          scenario === "reopen" ||
          scenario === "external-desktop-after-host"
        ) {
          assert.equal(error, null);
          assert.equal(settlement.failure, null);
          assert.equal(settlement.configurationGuardPassed, true);
          assert.equal(
            settlement.codexHomeUnchangedExactly,
            scenario !== "external-desktop-after-host",
          );
          assert.deepEqual(fixture.phases, scenario === "reopen" ? ["A", "B"] : ["A"]);
        } else {
          assert.ok(error instanceof Error);
          assert.notEqual(settlement.failure, null);
          assert.ok(fixture.phases.length <= 1);
        }
        if (scenario.startsWith("profile-")) {
          assert.equal(readFileSync(fixture.plan.profile, "utf8"), fixture.ownerProfile);
          assert.equal(settlement.codexHomeUnchangedExactly, false);
        }
        if (scenario.startsWith("config-")) {
          assert.equal(readFileSync(fixture.plan.codex_config, "utf8"), fixture.ownerConfig);
          assert.equal(settlement.codexHomeUnchangedExactly, false);
        }
        if (["profile-before-host", "config-before-host", "inspection-expired"].includes(scenario))
          assert.deepEqual(fixture.phases, []);
        if (scenario === "early-exit")
          assert.match(error?.message ?? "", /exited before an explicit lifecycle directive/);
        if (scenario === "inspection-expired")
          assert.match(error?.message ?? "", /allocation deadline expired at launch/);
        if (scenario === "inventory-timeout")
          assert.equal(objectSchema.parse(settlement.failure).class, "TimeoutExpired");
      } finally {
        rmSync(root, { recursive: true, force: true });
      }
    },
  );
}

test("a mismatched Claude path rejects before creating any child or requester", async (): Promise<void> => {
  const root = createFixtureDirectory("lifecycle-path-mismatch");
  try {
    const fixture = new LifecycleFixture(root, "stop", true);
    const error = await fixture.drive();
    assert.match(
      error?.message ?? "",
      /planned Claude executable differs from qualified Host PATH selection/,
    );
    assert.deepEqual(fixture.launched, []);
    assert.equal(existsSync(join(fixture.plan.evidence, "lifecycle-settlement.json")), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test(
  "a harmless recorded child is released with fresh identity checks",
  { timeout: 10_000 },
  async (): Promise<void> => {
    const owned = await harmlessChild();
    try {
      const pid = owned.child.pid;
      if (pid === undefined) throw new Error("fixture PID unavailable");
      const identity = processIdentitySchema.parse({ ...owned.identity, parent: process.pid });
      const result = await stopOwned(owned.child, new Map([[pid, identity]]));
      assert.deepEqual(result.remaining, {});
      assert.deepEqual(result.errors, []);
      assert.notEqual(result.exit, null);
    } finally {
      await releaseHarmlessChild(owned.child, owned.identity);
    }
  },
);

test(
  "denied observation retains unverified inventory and never sends a signal",
  { timeout: 10_000 },
  async (): Promise<void> => {
    const owned = await harmlessChild();
    try {
      const pid = owned.child.pid;
      if (pid === undefined) throw new Error("fixture PID unavailable");
      const identity = processIdentitySchema.parse({ ...owned.identity, parent: process.pid });
      const result = await stopOwned(owned.child, new Map([[pid, identity]]), {
        ...processOperations,
        table: (): never => {
          throw Object.assign(new Error("controlled denied observation"), { code: "EPERM" });
        },
      });
      assert.equal(typeof result.remaining[String(pid)], "string");
      assert.deepEqual(result.signals, []);
      assert.ok(result.errors.length > 0);
      assert.equal(owned.child.exitCode, null);
    } finally {
      await releaseHarmlessChild(owned.child, owned.identity);
    }
  },
);

test(
  "skipping inventory polling cannot default a retained child to successful cleanup",
  { timeout: 10_000 },
  async (): Promise<void> => {
    const root = createFixtureDirectory("inventory-no-poll");
    const fixture = new LifecycleFixture(root, "stop");
    const owned = await harmlessChild();
    try {
      const pid = owned.child.pid;
      if (pid === undefined) throw new Error("fixture PID unavailable");
      const plan = { ...fixture.plan, evidence: root };
      const inventory = join(root, "owned.json");
      saveRecord(inventory, {
        identities: { [String(pid)]: { ...owned.identity, parent: process.pid } },
      });
      const ticks = [0, 100_000, 200_000, 300_000];
      await assert.rejects(
        settleInventory(plan, inventory, {
          ...processOperations,
          monotonicNow: (): number => {
            const tick = ticks.shift();
            if (tick === undefined) throw new Error("unexpected extra inventory poll");
            return tick;
          },
        }),
        /cleanup incomplete/,
      );
      const settlement = readObservedJson(readFileSync(join(root, "owned-settlement.json")));
      assert.ok(Object.hasOwn(objectSchema.parse(settlement.remaining), String(pid)));
    } finally {
      await releaseHarmlessChild(owned.child, owned.identity);
      rmSync(root, { recursive: true, force: true });
    }
  },
);

test("a reaped initial child cannot authorize a new process identity capture", async (): Promise<void> => {
  const child = spawn(process.execPath, ["-e", ""], { stdio: "ignore" });
  await once(child, "exit");
  let queried = false;
  assert.throws(
    () =>
      observeChild(child, new Map(), {
        ...processOperations,
        table: (): never => {
          queried = true;
          throw new Error("must not inspect a reused PID");
        },
      }),
    /exited before identity acquisition/,
  );
  assert.equal(queried, false);
});

test(
  "native metadata lookup failure still closes and reaps the owned stdin carrier",
  { timeout: 10_000 },
  async (): Promise<void> => {
    const child = spawn(
      process.execPath,
      [new URL("../fixtures/native-read-carrier.ts", import.meta.url).pathname],
      { stdio: ["pipe", "pipe", "pipe"], detached: true },
    );
    await once(child, "spawn");
    await assert.rejects(
      readCarrierCorrelation(
        child,
        { thread_id: "fixture-thread", turn_id: "fixture-turn", push_id: "fixture-push" },
        (): never => {
          throw Object.assign(new Error("controlled metadata failure"), { code: "EPERM" });
        },
      ),
      /controlled metadata failure/,
    );
    assert.equal(child.exitCode, 0);
  },
);

test(
  "an actual local native pipe drives only the original synthetic read protocol",
  { timeout: 10_000 },
  async (): Promise<void> => {
    lookupProcessIdentity(process.pid);
    const child = spawn(
      process.execPath,
      [new URL("../fixtures/native-read-carrier.ts", import.meta.url).pathname],
      { stdio: ["pipe", "pipe", "pipe"], detached: true },
    );
    await once(child, "spawn");
    const result = await readCarrierCorrelation(child, {
      thread_id: "fixture-thread",
      turn_id: "fixture-turn",
      push_id: "fixture-push",
    });
    assert.equal(result.supported, true);
    assert.equal(child.exitCode, 0);
    await waitForProcess(child, 5000);
  },
);

// macOS installs /bin/ps setuid root, so a user-launched ps runs with effective UID 0 and the
// launching account's real UID. Hosts without that split cannot exercise this case through ps.
function setuidSystemPsUnavailable(): string | false {
  const status = statSync("/bin/ps");
  return status.uid === 0 && (status.mode & 0o4000) !== 0
    ? false
    : "host /bin/ps is not setuid root, so no effective/real UID split is observable";
}

test(
  "a user-launched setuid system utility stays owned by its real user in table, lookup and guard",
  { timeout: 15_000, skip: setuidSystemPsUnavailable() },
  async (): Promise<void> => {
    // Repeated comm columns exceed what the pipe holds, so the actual setuid ps stays blocked in
    // write until drained; only backpressure matters here, not other processes' arguments.
    const held = spawn(
      "/bin/ps",
      ["-axww", ...Array.from({ length: 40 }, () => ["-o", "comm="]).flat()],
      { stdio: ["ignore", "pipe", "ignore"] },
    );
    const [firstChunk] = await once(held.stdout, "data");
    held.stdout.pause();
    assert.ok(Buffer.isBuffer(firstChunk));
    if (held.pid === undefined) throw new Error("held setuid utility PID unavailable");
    try {
      const known = new Map<number, ProcessIdentity>();
      observeOwned(process.pid, known);
      const admitted = known.get(held.pid);
      assert.ok(admitted !== undefined, "held setuid utility was not admitted as owned");
      assert.equal(admitted.realUid, process.getuid?.());
      assert.equal(admitted.effectiveUid, 0);
      const lookup = lookupProcessIdentity(held.pid);
      assert.equal(lookup?.realUid, process.getuid?.());
      assert.equal(lookup?.effectiveUid, 0);
      assert.equal(sameIdentity(lookup, identityRecord(admitted)), true);
    } finally {
      held.stdout.resume();
      await once(held, "close");
    }
    assert.equal(held.exitCode, 0);
  },
);
