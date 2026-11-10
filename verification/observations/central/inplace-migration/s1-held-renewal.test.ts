import assert from "node:assert/strict";
import { test } from "node:test";
import { validateSyntheticRenewal } from "./s1-held-renewal.ts";

const project = "01900000-0000-7000-8000-000000000001";
const holder = "01900000-0000-7000-8000-000000000002";
const other = "01900000-0000-7000-8000-000000000003";
const bounds = { project, holder, lease: 1n, start: 900n, end: 1200n };
const fixture = {
  kind: "renewalObserved",
  value: {
    database: project,
    holder,
    lease: 1n,
    requestStarted: 1000n,
    currentRenewalObserved: true,
    answer: {
      kind: "renewed",
      value: {
        recorded: { epoch: 1n, expiresAt: 2000n },
        current: { database: project, holder, epoch: 1n, expiresAt: 2000n },
        replayed: false,
      },
    },
  },
};
function observed(): typeof fixture {
  return structuredClone(fixture);
}

test("only one non-replayed matching D/M/N renewal inside the original Host window qualifies", (): void => {
  assert.equal(validateSyntheticRenewal({ ...bounds, rows: [observed()] }).value.lease, 1n);
  for (const changed of [
    { ...bounds, project: other },
    { ...bounds, holder: other },
    { ...bounds, lease: 2n },
    { ...bounds, start: 1001n },
  ])
    assert.throws(
      () => validateSyntheticRenewal({ ...changed, rows: [observed()] }),
      /D\/M\/N or root-3 Host window/,
    );
  const replayed = observed();
  replayed.value.answer.value.replayed = true;
  assert.throws(() => validateSyntheticRenewal({ ...bounds, rows: [replayed] }));
  const expired = observed();
  expired.value.answer.value.current.expiresAt = 999n;
  assert.throws(() => validateSyntheticRenewal({ ...bounds, rows: [expired] }), /Host window/);
});
test("raw authority or duplicate completion rows cannot masquerade as one observed renewal", (): void => {
  assert.throws(
    () => validateSyntheticRenewal({ ...bounds, rows: [{ kind: "renewalAuthorityRead" }] }),
    /one current renewal/,
  );
  assert.throws(
    () => validateSyntheticRenewal({ ...bounds, rows: [observed(), observed()] }),
    /one current renewal/,
  );
});
