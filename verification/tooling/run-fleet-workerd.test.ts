import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempDisposableSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

test("fleet launcher help and invalid arguments stop before tool lookup or filesystem mutation", () => {
  const temporary = mkdtempDisposableSync(join(tmpdir(), "fleet-launcher-arguments-"));
  try {
    const launcher = fileURLToPath(new URL("run-fleet-workerd.ts", import.meta.url));
    for (const arguments_ of [["--help"], ["-h"], [], ["--unknown"], ["one", "two"]]) {
      const result = spawnSync(process.execPath, [launcher, ...arguments_], {
        cwd: temporary.path,
        env: { ...process.env, PATH: "", WASM_BINDGEN: "/missing-bindgen-must-not-be-looked-up" },
        encoding: "utf8",
      });
      const help = arguments_.length === 1 && ["--help", "-h"].includes(arguments_[0] ?? "");
      assert.equal(result.status, help ? 0 : 2);
      assert.match(help ? result.stdout : result.stderr, /^Usage:.*fresh-evidence-directory/);
      assert.deepEqual(readdirSync(temporary.path), []);
    }
  } finally {
    temporary.remove();
  }
});
