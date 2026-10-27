import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

export function runFixture(_configuration: unknown): void {
  appendFileSync(z.string().parse(process.env.CLAUDE_FIXTURE_COUNT), "launch\n");
  writeFileSync(
    z.string().parse(process.env.CLAUDE_FIXTURE_RECORD),
    wireJson({
      arguments: process.argv.slice(2),
      working_directory: process.cwd(),
      base_url: process.env.ANTHROPIC_BASE_URL ?? null,
      auth_token: process.env.ANTHROPIC_AUTH_TOKEN ?? null,
    }),
  );
  if (!process.env.CLAUDE_FIXTURE_NO_RECEIPT) console.log("bg-fixture-receipt-001");
  if (process.env.CLAUDE_FIXTURE_STDERR) console.error(process.env.CLAUDE_FIXTURE_STDERR);
  const status = z
    .string()
    .regex(/^[+-]?\d(?:_?\d)*$/)
    .parse((process.env.CLAUDE_FIXTURE_EXIT ?? "0").trim());
  process.exitCode = Number(status.replaceAll("_", ""));
}
