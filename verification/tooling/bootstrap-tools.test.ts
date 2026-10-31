import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const cases = [
  { name: "empty cache installs the locked version", status: 0, installs: true },
  { name: "matching cache skips installation", seed: "0.9.137", status: 0, installs: false },
  { name: "wrong cache is replaced", seed: "0.1.0", status: 0, installs: true },
  {
    name: "failed installation does not use a stale binary",
    seed: "0.1.0",
    fail: true,
    status: 9,
    installs: true,
  },
  {
    name: "installer producing the wrong version is rejected",
    installed: "0.1.0",
    status: 1,
    installs: true,
  },
  {
    name: "malformed version cannot reach installation",
    declaration: "cargo-nextest latest cargo-nextest ci\n",
    status: 2,
    installs: false,
  },
  {
    name: "check mode does not install a missing tool",
    checkOnly: true,
    status: 2,
    installs: false,
  },
  {
    name: "an empty role cannot pass",
    declaration: "# no declared tools\n",
    status: 2,
    installs: false,
  },
] satisfies readonly {
  readonly name: string;
  readonly seed?: string;
  readonly fail?: boolean;
  readonly installed?: string;
  readonly declaration?: string;
  readonly checkOnly?: boolean;
  readonly status: number;
  readonly installs: boolean;
}[];

for (const entry of cases) {
  test(`tool bootstrap: ${entry.name}`, (): void => {
    const root = mkdtempSync(join(tmpdir(), "bootstrap-tools-"));
    try {
      mkdirSync(join(root, "bin"));
      const fixture = new URL("./fixtures/bootstrap-cargo-fixture.ts", import.meta.url);
      symlinkSync(fixture, join(root, "bin/cargo"));
      const installRoot = join(root, "tools");
      if ("seed" in entry) {
        mkdirSync(join(installRoot, "bin"), { recursive: true });
        symlinkSync(fixture, join(installRoot, "bin/cargo-nextest"));
        writeFileSync(join(installRoot, "bin/version"), entry.seed);
      }
      const versions = join(root, "versions.tsv");
      const calls = join(root, "calls");
      writeFileSync(
        versions,
        "declaration" in entry ? entry.declaration : "cargo-nextest 0.9.137 cargo-nextest ci\n",
      );
      const result = spawnSync(
        "bash",
        [
          new URL("../../scripts/tooling/bootstrap-tools.sh", import.meta.url).pathname,
          "ci",
          ...("checkOnly" in entry ? ["--check"] : []),
        ],
        {
          env: {
            ...process.env,
            PATH: `${join(root, "bin")}:${process.env.PATH ?? ""}`,
            ROUTER_TOOL_VERSIONS_FILE: versions,
            ROUTER_TOOL_INSTALL_ROOT: installRoot,
            INSTALL_CALLS: calls,
            ...("fail" in entry ? { FAIL_INSTALL: "1" } : {}),
            ...("installed" in entry ? { INSTALLED_VERSION: entry.installed } : {}),
          },
          encoding: "utf8",
        },
      );
      assert.equal(result.status, entry.status, result.stderr);
      assert.equal(existsSync(calls), entry.installs);
      if (entry.installs)
        assert.ok(readFileSync(calls, "utf8").includes("--version =0.9.137 --locked"));
      if ("installed" in entry) assert.match(result.stderr, /version verification failed/);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
