import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ownedClosure,
  parseProcessIdentity,
  ProcessIdentityError,
  remainingOwned,
  sameIdentity,
  signalEachOwned,
} from "./process-ownership.ts";

const rootIdentity = {
  pid: 10,
  parent: 1,
  realUid: 501,
  effectiveUid: 501,
  group: 10,
  birth: "root-birth",
  executable: "/owned/router",
};

const childIdentity = {
  pid: 11,
  parent: 10,
  realUid: 501,
  effectiveUid: 501,
  group: 10,
  birth: "child-birth",
  executable: "/owned/provider",
};

test("recorded reparented children remain owned while reused roots cannot acquire descendants", (): void => {
  const known = new Map([
    [10, rootIdentity],
    [11, childIdentity],
  ]);
  assert.deepEqual(
    [...remainingOwned(known, new Map([[11, { ...childIdentity, parent: 1 }]])).keys()],
    [11],
  );
  assert.deepEqual(
    [
      ...ownedClosure(
        10,
        new Map([[10, rootIdentity]]),
        new Map([
          [10, { ...rootIdentity, birth: "new-birth" }],
          [12, { ...childIdentity, pid: 12 }],
        ]),
      ),
    ],
    [],
  );
  assert.equal(sameIdentity({ ...rootIdentity, parent: 9 }, rootIdentity), true);
  assert.throws(() => sameIdentity(null, {}), /incomplete recorded process identity/);
});

test("one disappeared child does not skip the remaining independently guarded signals", (): void => {
  const known = new Map([
    [10, rootIdentity],
    [11, childIdentity],
  ]);
  const signalled: number[] = [];
  const result = signalEachOwned(
    known,
    (pid) => known.get(pid) ?? null,
    (pid): void => {
      if (pid === 10) throw Object.assign(new Error("disappeared"), { code: "ESRCH" });
      signalled.push(pid);
    },
  );
  assert.deepEqual(signalled, [11]);
  assert.deepEqual(result, { sent: [11], errors: [] });
});

test("process query denial, absence and malformed snapshots remain distinct", (): void => {
  assert.equal(parseProcessIdentity({ status: 1, stdout: "", stderr: "" }), null);
  assert.throws(
    () => parseProcessIdentity({ status: 1, stdout: "", stderr: "denied" }),
    (error: unknown) =>
      error instanceof ProcessIdentityError && error.category === "CalledProcessError",
  );
  assert.throws(
    () => parseProcessIdentity({ status: 0, stdout: "malformed", stderr: "" }),
    /incomplete current process identity/,
  );
  assert.deepEqual(
    parseProcessIdentity({
      status: 0,
      stdout: " 123 501 0 123 Mon Oct 26 10:20:30 2026 /owned/with  spaces/router\n",
      stderr: "",
    }),
    {
      pid: 123,
      realUid: 501,
      effectiveUid: 0,
      group: 123,
      birth: "Mon Oct 26 10:20:30 2026",
      executable: "/owned/with  spaces/router",
    },
  );
});

test("a recorded identity from the effective-UID collector is refused, never reinterpreted", (): void => {
  const { realUid, effectiveUid, ...rest } = rootIdentity;
  assert.throws(
    () => sameIdentity(rootIdentity, { ...rest, uid: effectiveUid }),
    /incomplete recorded process identity/,
  );
  assert.equal(realUid, 501);
});
