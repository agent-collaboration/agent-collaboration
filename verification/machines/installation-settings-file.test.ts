import assert from "node:assert/strict";
import { test } from "node:test";
import { readsLegacyProvidersFile } from "./installation.ts";

test("provider isolation targets the file each declared product version reads", (): void => {
  // Arrange: the old baseline release, the last legacy release, the first settings.jsonc release.
  const legacy = ["0.1.125", "0.1.141", "0.0.9"];
  const current = ["0.1.142", "0.1.145", "0.2.0", "1.0.0"];

  // Act and assert
  for (const version of legacy) assert.equal(readsLegacyProvidersFile(version), true, version);
  for (const version of current) assert.equal(readsLegacyProvidersFile(version), false, version);
  assert.throws(() => readsLegacyProvidersFile("0.1"), /semantic product version/);
  assert.throws(() => readsLegacyProvidersFile("latest"), /semantic product version/);
});
