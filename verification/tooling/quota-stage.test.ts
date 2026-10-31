import assert from "node:assert/strict";
import { test } from "node:test";
import {
  deriveQuotaPackageDirectories,
  isQuotaInputPath,
  quotaSupportInputs,
} from "./select-quota-stage.ts";

const repositoryRoot = "/workspace";
const harness = "codex-router-quota-reset-test-harness";
const metadata = {
  packages: [
    {
      name: harness,
      version: "1.0.0",
      manifest_path: `/workspace/crates/${harness}/Cargo.toml`,
    },
    { name: "state", version: "1.0.0", manifest_path: "/workspace/crates/state/Cargo.toml" },
    { name: "unused", version: "1.0.0", manifest_path: "/workspace/crates/unused/Cargo.toml" },
    { name: "external", version: "1.0.0", manifest_path: "/registry/external/Cargo.toml" },
  ],
};
const tree = [
  `${harness} v1.0.0 (/workspace/crates/${harness})`,
  "state v1.0.0 (/workspace/crates/state)",
  "external v1.0.0",
  "state v1.0.0 (/workspace/crates/state) (*)",
].join("\n");

test("selected Cargo packages include transitive inputs without workspace-only feature edges", (): void => {
  assert.deepEqual(deriveQuotaPackageDirectories(metadata, tree, repositoryRoot), [
    `crates/${harness}`,
    "crates/state",
  ]);
});

test("crate boundaries include removed files and literal newlines without matching sibling names", (): void => {
  const inputs = { directories: ["crates/state"], files: ["Cargo.lock"] };
  assert.equal(isQuotaInputPath("crates/state/src/removed.rs", inputs), true);
  assert.equal(isQuotaInputPath("crates/state/src/line\nbreak.rs", inputs), true);
  assert.equal(isQuotaInputPath("crates/state-extra/src/lib.rs", inputs), false);
  assert.equal(isQuotaInputPath("docs/plan.md", inputs), false);
  assert.equal(isQuotaInputPath("Cargo.lock", inputs), true);
  assert.equal(isQuotaInputPath("Cargo.lock.backup", inputs), false);
});

test("external adapters, embedded schema and invocation controls remain quota inputs", (): void => {
  for (const path of [
    "scripts/verification/tui/fixtures/fixture-child.ts",
    "scripts/verification/tui/pnpm-lock.yaml",
    "protocol-schemas/acp-protocol/stable-schema.json",
    ".cargo/config.toml",
    ".config/nextest.toml",
    ".github/workflows/ci.yml",
    "scripts/tooling/tool-versions.tsv",
  ])
    assert.equal(isQuotaInputPath(path, quotaSupportInputs), true, path);
  assert.equal(isQuotaInputPath("docs/wip/release-analysis.md", quotaSupportInputs), false);
  assert.equal(
    isQuotaInputPath("scripts/verification/tui-other/file.ts", quotaSupportInputs),
    false,
  );
});

test("a workspace SQLx offline query description alone selects the quota stage", (): void => {
  assert.equal(isQuotaInputPath(".sqlx/query-x.json", quotaSupportInputs), true);
});

test("a new extensionless root Rust toolchain file selects the quota stage", (): void => {
  assert.equal(isQuotaInputPath("rust-toolchain", quotaSupportInputs), true);
});

test("missing or malformed Cargo selection fails instead of claiming an unrelated change", (): void => {
  assert.throws(() => deriveQuotaPackageDirectories({}, tree, repositoryRoot));
  assert.throws(
    () => deriveQuotaPackageDirectories(metadata, "state v1.0.0", repositoryRoot),
    /harness is absent/,
  );
});

test("same-name sources need Cargo's displayed path to identify a repository dependency", (): void => {
  const ambiguous = {
    packages: [
      ...metadata.packages,
      { name: "state", version: "1.0.0", manifest_path: "/registry/state/Cargo.toml" },
    ],
  };
  assert.deepEqual(deriveQuotaPackageDirectories(ambiguous, tree, repositoryRoot), [
    `crates/${harness}`,
    "crates/state",
  ]);
  assert.throws(
    () =>
      deriveQuotaPackageDirectories(
        ambiguous,
        tree.replaceAll(" (/workspace/crates/state)", ""),
        repositoryRoot,
      ),
    /ambiguous package/,
  );
});
