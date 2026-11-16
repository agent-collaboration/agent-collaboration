import assert from "node:assert/strict";
import { test } from "node:test";
import { encode } from "../../../exact-json.ts";
import { decodeSavedCodexHomeSnapshot as decodeSavedConfiguration } from "../../runtime/readonly-codex-home.ts";

const fingerprint = {
  path: "/normal/config",
  sha256: "digest",
  size: 123n,
  modified_ns: 1794606343391933169n,
};
const snapshot = {
  profile: fingerprint,
  config: fingerprint,
  config_sections: [
    { header: "<root>", table: [], occurrence: 0, sha256: "section-digest", names_run: false },
  ],
};

test("saved guard snapshots round-trip occurrence counters without losing exact timestamps", (): void => {
  const decoded = decodeSavedConfiguration(encode(snapshot));
  assert.equal(decoded.config_sections[0]?.occurrence, 0);
  assert.equal(typeof decoded.config_sections[0]?.occurrence, "number");
  assert.equal(decoded.profile.modified_ns, 1794606343391933169n);
  assert.equal(decoded.profile.size, 123n);
  assert.equal(decoded.config_sections[0]?.sha256, "section-digest");
});

test("unsafe, negative and fractional occurrence indices cannot weaken the config guard", (): void => {
  for (const occurrence of [9007199254740992n, -1n, 0.5]) {
    assert.throws((): unknown =>
      decodeSavedConfiguration(
        encode({ ...snapshot, config_sections: [{ ...snapshot.config_sections[0], occurrence }] }),
      ),
    );
  }
});
