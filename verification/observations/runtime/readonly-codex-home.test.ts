import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { configurationUnchanged, type CodexHomeSnapshot } from "./readonly-codex-home.ts";
import { readSavedCodexHome } from "./runtime-configuration.ts";
import { saveRecord } from "./runtime-plan.ts";

const snapshot: CodexHomeSnapshot = {
  profile: {
    path: "/normal/profile",
    sha256: "profile-digest",
    size: 77n,
    modified_ns: 1794606343391933169n,
  },
  config: {
    path: "/normal/config",
    sha256: "config-digest",
    size: 123n,
    modified_ns: 1794606343391933170n,
  },
  config_sections: [
    { header: "<root>", table: [], occurrence: 0, sha256: "root-digest", names_run: false },
    {
      header: "[[profiles]]",
      table: ["profiles"],
      occurrence: 1,
      sha256: "second-digest",
      names_run: false,
    },
  ],
};

function savedEvidence(): string {
  const evidence = mkdtempSync(join(tmpdir(), "d0-codex-home-"));
  saveRecord(join(evidence, "codex-home-before.json"), snapshot);
  return evidence;
}

test("the runtime reads back the snapshot it saved, with exact fingerprint integers", () => {
  const read = readSavedCodexHome(savedEvidence());
  assert.deepEqual(read, snapshot);
  assert.equal(read.config.modified_ns, 1794606343391933170n);
  assert.equal(typeof read.config_sections[1]?.occurrence, "number");
});

test("a read-back snapshot still tells an unchanged configuration from a changed one", () => {
  const before = readSavedCodexHome(savedEvidence());
  const unchanged = {
    kind: "observed" as const,
    before,
    after: snapshot,
    config_changes: [],
    writer_attribution: "unattributed" as const,
  };
  assert.equal(configurationUnchanged(unchanged), true);
  const changed = { ...snapshot, config: { ...snapshot.config, sha256: "edited-digest" } };
  assert.equal(configurationUnchanged({ ...unchanged, after: changed }), false);
});
