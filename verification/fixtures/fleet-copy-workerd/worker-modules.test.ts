import assert from "node:assert/strict";
import { mkdtempDisposableSync, mkdirSync, writeFileSync, unlinkSync, renameSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { checkedWorkerModules, declaredModulePaths } from "./worker-modules.ts";

test("module manifest requires every actual generated module and the exact snippet hash", () => {
  const temporary = mkdtempDisposableSync(join(tmpdir(), "fleet-worker-modules-"));
  try {
    for (const path of declaredModulePaths) {
      mkdirSync(dirname(join(temporary.path, path)), { recursive: true });
      writeFileSync(join(temporary.path, path), "");
    }
    writeFileSync(join(temporary.path, "index.d.ts"), "metadata");
    assert.equal(checkedWorkerModules(temporary.path)[0]?.path, join(temporary.path, "shim.mjs"));
    writeFileSync(join(temporary.path, "extra.js"), "");
    assert.throws(() => checkedWorkerModules(temporary.path), /generated runtime modules/);
    unlinkSync(join(temporary.path, "extra.js"));
    renameSync(join(temporary.path, "index.js"), join(temporary.path, "changed.js"));
    assert.throws(() => checkedWorkerModules(temporary.path), /generated runtime modules/);
    renameSync(join(temporary.path, "changed.js"), join(temporary.path, "index.js"));
    mkdirSync(join(temporary.path, "snippets/worker-changed"));
    renameSync(
      join(temporary.path, declaredModulePaths[4]),
      join(temporary.path, "snippets/worker-changed/inline0.js"),
    );
    assert.throws(() => checkedWorkerModules(temporary.path), /generated runtime modules/);
  } finally {
    temporary.remove();
  }
});
