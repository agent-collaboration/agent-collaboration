import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

export function runFixture(_configuration: unknown): void {
  const invocation = {
    arguments: process.argv.slice(2),
    working_directory: process.cwd(),
    base_url: process.env.ANTHROPIC_BASE_URL ?? null,
    auth_token: process.env.ANTHROPIC_AUTH_TOKEN ?? null,
  };
  if (process.env.CLAUDE_FIXTURE_FLOW) {
    appendFileSync(process.env.CLAUDE_FIXTURE_FLOW, `${wireJson(invocation)}\n`);
    if (invocation.arguments[0] === "attach") {
      console.log("attached fixture session");
      return;
    }
    if (!invocation.arguments.includes("--bg") && !invocation.arguments.includes("--background")) {
      console.log("foreground fixture session");
      return;
    }
  }
  appendFileSync(z.string().parse(process.env.CLAUDE_FIXTURE_COUNT), "launch\n");
  writeFileSync(z.string().parse(process.env.CLAUDE_FIXTURE_RECORD), wireJson(invocation));
  if (!process.env.CLAUDE_FIXTURE_NO_RECEIPT) {
    if (process.env.CLAUDE_FIXTURE_RECEIPT)
      process.stdout.write(process.env.CLAUDE_FIXTURE_RECEIPT);
    else console.log("bg-fixture-receipt-001");
  }
  if (process.env.CLAUDE_FIXTURE_STDERR) console.error(process.env.CLAUDE_FIXTURE_STDERR);
  const status = z
    .string()
    .regex(/^[+-]?\d(?:_?\d)*$/)
    .parse((process.env.CLAUDE_FIXTURE_EXIT ?? "0").trim());
  process.exitCode = Number(status.replaceAll("_", ""));
}
