import { test } from "node:test";
import assert from "node:assert/strict";
import { validateUpgradeEnrollment } from "./installation.ts";
const installationId = "01900000-0000-7000-8000-000000000001",
  machineId = "01900000-0000-7000-8000-000000000002";
const expected = {
  root: "/private/fixture/root",
  installationId,
  machineId,
  endpoint: "https://staging.example.test",
  enrollmentSha256: "a".repeat(64),
  baselineId: "01900000-0000-7000-8000-000000000003",
};
const identity = { serviceId: installationId, peerId: machineId, machineLabel: "synthetic" };
const status = {
  status: { kind: "valid", enrollment: { peer: machineId, endpoint: expected.endpoint } },
};
test("upgrade requires original identity and exact enrollment document before reusing a root", (): void => {
  validateUpgradeEnrollment({
    expected,
    identity,
    status,
    enrollmentSha256: expected.enrollmentSha256,
  });
  assert.throws(
    () =>
      validateUpgradeEnrollment({ expected, identity, status, enrollmentSha256: "b".repeat(64) }),
    /differs/,
  );
  assert.throws(
    () =>
      validateUpgradeEnrollment({
        expected,
        identity: { ...identity, serviceId: machineId },
        status,
        enrollmentSha256: expected.enrollmentSha256,
      }),
    /differs/,
  );
  assert.throws(() =>
    validateUpgradeEnrollment({
      expected,
      identity,
      status: { status: { kind: "absent" } },
      enrollmentSha256: expected.enrollmentSha256,
    }),
  );
});
