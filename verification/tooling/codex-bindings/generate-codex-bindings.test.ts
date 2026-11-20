import assert from "node:assert/strict";
import { copyFileSync, mkdtempDisposableSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  checkCodexBindings,
  readPinnedCodexInput,
  renderCodexModelList,
} from "./generate-codex-bindings.ts";

const schemas = fileURLToPath(new URL("../../schemas/", import.meta.url));
const committed = fileURLToPath(
  new URL("../../../crates/vendor-protocol/src/codex/generated.rs", import.meta.url),
);
const schema = readPinnedCodexInput(schemas, "codexAppServer");
const pairing = readPinnedCodexInput(schemas, "codexAppServerPairing");

test("two offline generations match the committed model/list closure", (): void => {
  const generated = renderCodexModelList(schema, pairing);
  assert.equal(renderCodexModelList(schema, pairing), generated);
  assert.equal(generated.match(/(?:^    \w+ \{|^pub (?:enum|struct) \w+)/gm)?.length, 12);
  checkCodexBindings(committed, generated);
});

test("offline drift check rejects an edited artifact and leaves its bytes intact", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-binding-"));
  const output = join(scratch.path, "generated.rs");
  const edited = readFileSync(committed, "utf8").replace(
    'rename = "nextCursor"',
    'rename = "wrongCursor"',
  );
  writeFileSync(output, edited);

  assert.throws(
    () => checkCodexBindings(output, renderCodexModelList(schema, pairing)),
    /generated Codex bindings differ/,
  );
  assert.equal(readFileSync(output, "utf8"), edited);
});

test("changed schema and a pairing pointed at another existing type fail the pins", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-inputs-"));
  copyFileSync(join(schemas, "pins.json"), join(scratch.path, "pins.json"));
  const alteredSchema = `${schema}\n`;
  const alteredPairing = pairing.replace(
    '"responseDefinition": "#/definitions/v2/ModelListResponse"',
    '"responseDefinition": "#/definitions/v2/ModelProviderCapabilitiesReadResponse"',
  );
  writeFileSync(join(scratch.path, "codex-app-server-0.166.0.experimental.json"), alteredSchema);
  writeFileSync(join(scratch.path, "codex-app-server-0.166.0.pairing.json"), alteredPairing);

  assert.throws(() => readPinnedCodexInput(scratch.path, "codexAppServer"), /SHA-256 mismatch/);
  assert.throws(
    () => readPinnedCodexInput(scratch.path, "codexAppServerPairing"),
    /SHA-256 mismatch/,
  );
  assert.throws(
    () => renderCodexModelList(schema, alteredPairing),
    /pairing pointers disagree with declared Rust types/,
  );
});

test("semantic validation refuses unsupported constraints and unresolved payload references", (): void => {
  const unsupported = schema.replace(
    '"title": "ModelListParams",',
    '"title": "ModelListParams", "minProperties": 1,',
  );
  const unresolved = schema.replace('"ModelUpgradeInfo": {', '"RenamedUpgradeInfo": {');

  assert.throws(
    () => renderCodexModelList(unsupported, pairing),
    /ModelListParams: unsupported definition.*minProperties/,
  );
  assert.throws(
    () => renderCodexModelList(unresolved, pairing),
    /ModelUpgradeInfo: missing definition/,
  );
});
