// Owner access and Router refusal while the migration Worker is deployed.

import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { ownerClaims, testSigner, type TestSigner } from "./support/owner_assertion.ts";
import { persistenceRoot, startWorker } from "./support/workerd_harness.ts";

let signer: TestSigner;
let storage: { path: string; remove(): Promise<void> };

before(async () => {
  signer = await testSigner();
  storage = await persistenceRoot();
});
after(async () => {
  await storage.remove();
});

test("only the owner reaches an admin call, and Routers are refused while it is deployed", async () => {
  // Arrange
  const migration = await startWorker("migration", storage.path, signer.jwks);
  const now = Math.floor(Date.now() / 1000);
  const call = (path: string, assertion?: string) =>
    migration.dispatchFetch(`https://central.test${path}`, {
      headers: assertion === undefined ? {} : { "Cf-Access-Jwt-Assertion": assertion },
    });

  try {
    // Act and assert: no assertion, another identity, an expired owner.
    const missing = await call("/admin/v1/migration/inventory");
    assert.deepEqual(
      [missing.status, await missing.json()],
      [403, { refused: "missingAssertion" }],
    );
    const stranger = await call(
      "/admin/v1/migration/inventory",
      await signer.sign(ownerClaims(now, "someone@example.test")),
    );
    assert.deepEqual([stranger.status, await stranger.json()], [403, { refused: "wrongOwner" }]);
    const expired = await call(
      "/admin/v1/migration/inventory",
      await signer.sign({ ...ownerClaims(now), exp: now - 1 }),
    );
    assert.deepEqual([expired.status, await expired.json()], [403, { refused: "expired" }]);
    // A Router's session and central's own admin routes are refused.
    const session = await call("/v1/session");
    assert.equal(session.status, 503);
    const directory = await call("/admin/v1/directory", await signer.sign(ownerClaims(now)));
    assert.equal(directory.status, 503);
    // Migrate accepts only POST.
    const migrateByGet = await call(
      "/admin/v1/migration/migrate",
      await signer.sign(ownerClaims(now)),
    );
    assert.equal(migrateByGet.status, 405);
  } finally {
    await migration.dispose();
  }
});
