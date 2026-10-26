import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import {
  allowedSectionChange,
  compareSections,
  fingerprintSections,
  sectionChangeSchema,
  sectionFingerprintSchema,
} from "../config-sections.ts";
import {
  ConfigurationChanged,
  configurationGuardPassed,
  configurationReferences,
  configurationUnchanged,
  fingerprint,
  observeConfiguration,
  registerGuardIdentifiers,
  requireReadOnlyConfiguration,
  requireTrustedNativeCwd,
  snapshotConfiguration,
} from "../readonly-codex-home.ts";

const caseSchema = z.object({
  name: z.string(),
  before: z.string(),
  after: z.string(),
  references: z.array(z.string()),
  expected: z.object({
    before: z.array(sectionFingerprintSchema),
    after: z.array(sectionFingerprintSchema),
    changes: z.array(sectionChangeSchema),
    allowed: z.boolean(),
  }),
});
const baselineDirectory = new URL("baselines/", import.meta.url);
for (const name of readdirSync(baselineDirectory)
  .filter((name) => name.startsWith("config-"))
  .sort()) {
  const entry = caseSchema.parse(
    JSON.parse(readFileSync(new URL(name, baselineDirectory), "utf8")),
  );
  test(`configuration ${entry.name} matches original section hashes and attribution`, (): void => {
    const before = fingerprintSections(Buffer.from(entry.before), entry.references);
    const after = fingerprintSections(Buffer.from(entry.after), entry.references);
    const changes = compareSections(before, after);
    assert.deepEqual(
      { before, after, changes, allowed: changes.every(allowedSectionChange) },
      entry.expected,
    );
  });
}

test("saved profiles and rejected configuration edits are preserved byte for byte", (): void => {
  const root = createFixtureDirectory("readonly-config");
  try {
    const profile = join(root, "profile");
    const config = join(root, "config");
    writeFileSync(profile, "saved profile");
    writeFileSync(config, 'model = "fixed"\n');
    const before = snapshotConfiguration(profile, config);
    writeFileSync(config, 'model = "changed"\n');
    const observation = observeConfiguration({ before, profile, config });
    assert.equal(configurationGuardPassed(observation), false);
    assert.throws(() => requireReadOnlyConfiguration(observation), ConfigurationChanged);
    assert.equal(readFileSync(profile, "utf8"), "saved profile");
    assert.equal(readFileSync(config, "utf8"), 'model = "changed"\n');
    assert.deepEqual(fingerprint(profile), before.profile);
    writeFileSync(
      config,
      'model = "fixed"\n[desktop.preferences]\nsecret = "PRIVATE_VALUE_CANARY"\n',
    );
    const external = observeConfiguration({ before, profile, config });
    assert.equal(configurationGuardPassed(external), true);
    assert.equal(configurationUnchanged(external), false);
    assert.ok(
      !external.before.config_sections.some((section) =>
        section.header.includes("PRIVATE_VALUE_CANARY"),
      ),
    );
    writeFileSync(profile, "owner edit");
    assert.equal(
      configurationGuardPassed(observeConfiguration({ before, profile, config })),
      false,
    );
    assert.equal(readFileSync(profile, "utf8"), "owner edit");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("native workspaces require existing exact trust and registered identities remain protected", (): void => {
  const root = createFixtureDirectory("config-identities");
  try {
    const profile = join(root, "profile");
    const config = join(root, "config");
    writeFileSync(profile, "saved profile");
    writeFileSync(config, 'model = "fixed"\n');
    assert.throws(() => requireTrustedNativeCwd(config, root), /existing exact trusted entry/);
    assert.equal(readFileSync(config, "utf8"), 'model = "fixed"\n');
    const trusted = `[projects."${root}"]\ntrust_level = "trusted"\n`;
    writeFileSync(config, trusted);
    requireTrustedNativeCwd(config, root);
    const before = snapshotConfiguration(profile, config, [root]);
    registerGuardIdentifiers(root, { target: { sessionId: "actual-new-run-session" } });
    const references = configurationReferences(root, [root]);
    writeFileSync(config, `${trusted}[desktop.preferences]\nsession = "actual-new-run-session"\n`);
    assert.equal(
      configurationGuardPassed(observeConfiguration({ before, profile, config, references })),
      false,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("invalid TOML stays untouched and is reported as unreadable", (): void => {
  const root = createFixtureDirectory("config-invalid");
  try {
    const profile = join(root, "profile");
    const config = join(root, "config");
    writeFileSync(profile, "saved profile");
    writeFileSync(config, 'model = "fixed"\n');
    const before = snapshotConfiguration(profile, config);
    for (const invalid of ["invalid = [", "[desktop\nflag = true\n"]) {
      writeFileSync(config, invalid);
      const observation = observeConfiguration({ before, profile, config });
      assert.equal(observation.kind, "unreadable");
      assert.equal(configurationGuardPassed(observation), false);
      assert.equal(readFileSync(config, "utf8"), invalid);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
