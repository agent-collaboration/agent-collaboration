import assert from "node:assert/strict";
import { test } from "node:test";
import type { Miniflare } from "miniflare";
import {
  assertHeartbeatDidNotWake,
  assertRestoredFrame,
  runHibernationFixture,
  assertEvictionDidNotReconstruct,
  assertSingleDecidingFrame,
  assertSingleWelcome,
  assertDecidingConstructor,
  classifyFixtureOutcome,
} from "./durable-object-hibernation.ts";

const first = {
  kind: "observed",
  instance: "01890f3e-7b8c-7cc0-8000-000000000001",
  attachment: { witness: "01890f3e-7b8c-7cc0-8000-000000000002" },
  count: 1,
};
const second = {
  ...first,
  instance: "01890f3e-7b8c-7cc0-8000-000000000003",
  count: 2,
};

test("a wrapper that disposes before throwing still records completed disposal", async (): Promise<void> => {
  const receipt = await runHibernationFixture({
    fixture: "hibernating",
    createRuntime: async (construct: () => Miniflare): Promise<Miniflare> => {
      const runtime = construct();
      await runtime.dispose();
      throw new Error("deliberate wrapper rejection after disposal");
    },
  });
  assert.equal(receipt.failure, "deliberate wrapper rejection after disposal");
  assert.equal(receipt.disposed, true, "awaited wrapper disposal must be reported");
  assert.deepEqual(receipt.disposition, { kind: "failed", exitCode: 1 });
});

test("heartbeat oracle rejects either constructor or handler execution", (): void => {
  const before = { constructors: 1, handlers: 1 };
  assertHeartbeatDidNotWake(before, before);
  assert.throws(
    (): void => assertHeartbeatDidNotWake(before, { constructors: 2, handlers: 1 }),
    /heartbeat must add no observed constructor or handler witnesses/,
  );
  assert.throws(
    (): void => assertHeartbeatDidNotWake(before, { constructors: 1, handlers: 2 }),
    /heartbeat must add no observed constructor or handler witnesses/,
  );
});

test("restoration requires new instance, exact attachment and surviving store", (): void => {
  const verify = (after: unknown): void =>
    assertRestoredFrame({ before: first, after, witness: first.attachment.witness });
  verify(second);
  assert.throws(
    (): void => verify({ ...second, instance: first.instance }),
    /real frame must run a new object instance/,
  );
  assert.throws(
    (): void => verify({ ...second, attachment: { witness: second.instance } }),
    /original attachment must restore/,
  );
  assert.throws(
    (): void => verify({ ...second, count: 1 }),
    /same owning SQLite state must survive eviction/,
  );
  assert.throws(
    (): void => verify({ ...second, count: 3 }),
    /same owning SQLite state must survive eviction/,
  );
  assert.throws(
    (): void => verify({ ...second, attachment: null }),
    /Invalid input: expected object, received null/,
  );
});

test("eviction refuses any observed constructor or handler execution", (): void => {
  const before = { constructors: 1, handlers: 1 };
  assertEvictionDidNotReconstruct(before, before);
  for (const after of [
    { constructors: 2, handlers: 1 },
    { constructors: 1, handlers: 2 },
  ]) {
    assert.throws(
      (): void => assertEvictionDidNotReconstruct(before, after),
      /eviction must run neither constructor nor handler/,
    );
  }
});

test("deciding frame requires exactly one constructor and one handler", (): void => {
  const before = { constructors: 1, handlers: 1 };
  assertSingleDecidingFrame(before, { constructors: 2, handlers: 2 });
  for (const after of [
    { constructors: 1, handlers: 2 },
    { constructors: 3, handlers: 2 },
    { constructors: 2, handlers: 1 },
    { constructors: 2, handlers: 3 },
  ]) {
    assert.throws(
      (): void => assertSingleDecidingFrame(before, after),
      /deciding frame must run exactly one constructor and handler/,
    );
  }
});

test("the original socket must receive exactly one Welcome", (): void => {
  assertSingleWelcome(1);
  for (const count of [0, 2]) {
    assert.throws((): void => assertSingleWelcome(count), /no reconnect or replacement Welcome/);
  }
});

test("the serving constructor witness must arrive during the deciding phase", (): void => {
  assertDecidingConstructor([{ instance: second.instance, phase: "deciding" }], second.instance);
  for (const phase of ["connecting", "baseline", "evicting", "heartbeat", "settling"] as const) {
    assert.throws(
      (): void =>
        assertDecidingConstructor([{ instance: second.instance, phase }], second.instance),
      /serving constructor witness must arrive after observe dispatch/,
    );
  }
  assert.throws(
    (): void => assertDecidingConstructor([], second.instance),
    /serving instance must have exactly one constructor witness/,
  );
  assert.throws(
    (): void =>
      assertDecidingConstructor(
        [
          { instance: second.instance, phase: "deciding" },
          { instance: second.instance, phase: "deciding" },
        ],
        second.instance,
      ),
    /serving instance must have exactly one constructor witness/,
  );
});

test("only the exact active-reference eviction failure is the required red", (): void => {
  const intended = {
    fixture: "non-hibernating",
    disposed: true,
    failure: "Timed out waiting to evict Durable Object: it still has active references.",
    stages: ["ready", "original-socket-welcome", "owning-store-first-frame", "eviction-requested"],
  } as const;
  assert.deepEqual(classifyFixtureOutcome(intended), { kind: "expected-red", exitCode: 1 });
  for (const invalid of [
    { ...intended, disposed: false },
    { ...intended, failure: null },
    { ...intended, failure: "original socket closed before deciding frame" },
    { ...intended, stages: [] },
    { ...intended, stages: ["eviction-requested"] },
    { ...intended, stages: [...intended.stages, "target-evicted"] },
  ]) {
    assert.deepEqual(classifyFixtureOutcome(invalid), { kind: "invalid-control", exitCode: 2 });
  }
  assert.deepEqual(
    classifyFixtureOutcome({
      fixture: "hibernating",
      stages: ["same-socket-new-instance-restored-attachment-and-store"],
      failure: null,
      disposed: true,
    }),
    { kind: "passed", exitCode: 0 },
  );
  assert.deepEqual(
    classifyFixtureOutcome({ fixture: "hibernating", stages: [], failure: null, disposed: true }),
    { kind: "failed", exitCode: 1 },
  );
  assert.deepEqual(
    classifyFixtureOutcome({
      fixture: "hibernating",
      stages: [],
      failure: "broken",
      disposed: true,
    }),
    { kind: "failed", exitCode: 1 },
  );
});
