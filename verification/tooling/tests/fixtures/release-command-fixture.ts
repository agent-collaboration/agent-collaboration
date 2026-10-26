#!/usr/bin/env node
import { createHash } from "node:crypto";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { z } from "zod";

const fixtureSchema = z.object({
  root: z.string(),
  log: z.string(),
  version: z.string(),
  commit: z.string(),
  asset: z.string(),
  importerVersion: z.string().optional(),
});

function main(): number {
  const planPath = z.string().parse(process.env.RELEASE_COMMAND_FIXTURE);
  const fixture = fixtureSchema.parse(JSON.parse(readFileSync(planPath, "utf8")));
  const tool = basename(z.string().parse(process.argv[1]));
  const args = process.argv.slice(2);
  appendFileSync(fixture.log, `${JSON.stringify({ tool, args })}\n`);
  if (tool === "git") {
    if (args[0] === "rev-parse") console.log(fixture.commit);
    if (args[0] === "diff") console.log("Formula/codex-router.rb");
  } else if (tool === "gh") {
    const assetName = `codex-router-v${fixture.version}-aarch64-apple-darwin.tar.gz`;
    if (args[0] === "api")
      console.log(
        JSON.stringify({
          assets: [
            {
              name: assetName,
              digest: `sha256:${createHash("sha256").update(fixture.asset).digest("hex")}`,
            },
          ],
        }),
      );
    else {
      const directory = z.string().parse(args[args.indexOf("--dir") + 1]);
      writeFileSync(join(directory, assetName), fixture.asset);
    }
  } else if (tool === "brew") {
    if (args[0] === "--repository") console.log(join(fixture.root, "tap"));
    if (args[0] === "--prefix") console.log(join(fixture.root, "install"));
    if (args[0] === "list") console.log("codex-router 0.1.101");
  } else if (tool === "file") console.log("Mach-O 64-bit executable arm64");
  else if (tool === "codesign" && args[0] === "-dv")
    console.error(
      `Identifier=dev.shravansunder.${basename(z.string().parse(args.at(-1)))}\nTeamIdentifier=974QD84WVC`,
    );
  else if (
    [
      "codex-router",
      "agent-collaboration",
      "agent-sessions",
      "codex-router-board-upgrade",
    ].includes(tool)
  )
    console.log(
      `${tool} ${tool === "codex-router-board-upgrade" ? (fixture.importerVersion ?? fixture.version) : fixture.version}`,
    );
  else if (tool !== "ruby" && tool !== "codesign")
    throw new Error(`unexpected release fixture command ${tool}`);
  return 0;
}

if (import.meta.main) process.exitCode = main();
