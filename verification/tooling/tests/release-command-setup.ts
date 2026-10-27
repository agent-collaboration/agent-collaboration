import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { installedExecutables } from "../publish-homebrew-tap-local.ts";

export function releaseCommandFixture(
  root: string,
  importerVersion: string = "0.1.102",
): {
  readonly environment: NodeJS.ProcessEnv;
  readonly formula: string;
  readonly log: string;
  readonly sha256: string;
} {
  const fixture = new URL("fixtures/release-command-fixture.ts", import.meta.url).pathname;
  mkdirSync(join(root, "bin"));
  mkdirSync(join(root, "tap/Formula"), { recursive: true });
  mkdirSync(join(root, "install/bin"), { recursive: true });
  for (const name of ["git", "gh", "brew", "ruby", "file", "codesign"])
    symlinkSync(fixture, join(root, "bin", name));
  for (const name of installedExecutables) symlinkSync(fixture, join(root, "install/bin", name));
  const formula = join(root, "tap/Formula/codex-router.rb");
  writeFileSync(
    formula,
    readFileSync(new URL("fixtures/current-formula.rb", import.meta.url), "utf8").replaceAll(
      "v0.1.102",
      "v0.1.101",
    ),
  );
  const log = join(root, "commands.jsonl");
  const asset = "public release fixture bytes";
  const plan = join(root, "plan.json");
  writeFileSync(
    plan,
    JSON.stringify({
      root,
      log,
      version: "0.1.102",
      commit: "be7e61e8bca32ebac172eb743e8064d2c7d6ffe7",
      asset,
      importerVersion,
    }),
  );
  return {
    formula,
    log,
    sha256: createHash("sha256").update(asset).digest("hex"),
    environment: {
      ...process.env,
      PATH: `${join(root, "bin")}:${process.env.PATH ?? ""}`,
      RELEASE_COMMAND_FIXTURE: plan,
    },
  };
}
