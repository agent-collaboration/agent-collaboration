import { execFileSync } from "node:child_process";
import { readFile, realpath } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { parse } from "smol-toml";
import { z } from "zod";
import type { RunSource } from "./declaration.ts";
import { ObservationOpen, type Receipt } from "./receipt.ts";

function lockGraph(text: string): unknown {
  const value = z
    .object({
      package: z.array(
        z
          .object({
            dependencies: z.array(z.string()).optional(),
          })
          .passthrough(),
      ),
    })
    .passthrough()
    .parse(parse(text));
  return {
    ...value,
    package: value.package.map((entry) =>
      entry.dependencies === undefined
        ? entry
        : { ...entry, dependencies: [...entry.dependencies].sort() },
    ),
  };
}

export async function checkSource(declaration: RunSource, receipt: Receipt): Promise<void> {
  if ((await realpath(fileURLToPath(new URL("..", import.meta.url)))) !== declaration.repository)
    throw new ObservationOpen("declaration repository differs from the running harness");
  const git = (arguments_: readonly string[]): string =>
    execFileSync("git", arguments_, {
      cwd: declaration.repository,
      encoding: "utf8",
      timeout: 10_000,
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  // Discovery permits inherited unbuilt test/example/Python work. Executed
  // artifacts remain frozen by hash/signature. Confirmation requires the clean tree.
  const discoveryExclusions =
    declaration.purpose === "discovery"
      ? [
          ":(exclude)**/tests/**",
          ":(exclude)**/examples/**",
          ":(exclude)crates/codex-router-test-support",
          ":(exclude)scripts/verification/*.py",
          ":(exclude)Cargo.lock",
        ]
      : [];
  const sourcePaths = [
    ".",
    ":(exclude)docs",
    ":(exclude)tmp",
    ":(exclude).agents",
    ":(exclude).github",
    ...discoveryExclusions,
  ];
  const checkoutHead = git(["rev-parse", "HEAD"]);
  if (checkoutHead !== declaration.harnessPin)
    git(["diff", "--quiet", declaration.harnessPin, checkoutHead, "--", ...sourcePaths]);
  if (
    git([
      "status",
      "--porcelain",
      "--",
      ".",
      ":(exclude)docs",
      ":(exclude)tmp",
      ":(exclude).agents",
      ":(exclude).github",
      ...discoveryExclusions,
    ]) !== ""
  )
    throw new ObservationOpen("harness source is not the declared immutable pin");
  git([
    "diff",
    "--quiet",
    declaration.productPin,
    declaration.harnessPin,
    "--",
    ".",
    ":(exclude)verification",
    ":(exclude)docs",
    ":(exclude)tmp",
    ":(exclude).agents",
    ":(exclude).github",
    ...discoveryExclusions,
  ]);
  if (
    declaration.purpose === "discovery" &&
    !isDeepStrictEqual(
      lockGraph(git(["show", `${declaration.productPin}:Cargo.lock`])),
      lockGraph(await readFile(`${declaration.repository}/Cargo.lock`, "utf8")),
    )
  )
    throw new ObservationOpen("discovery dependency graph differs from the frozen product");
  if (Date.parse(declaration.deadline) <= Date.now())
    throw new ObservationOpen("declared run deadline has passed");
  await receipt.record("sourceInputsChecked", {
    declaredHarnessPin: declaration.harnessPin,
    checkoutHead,
    unchangedExecutedInputs: true,
  });
}
