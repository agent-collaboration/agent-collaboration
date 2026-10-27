import assert from "node:assert/strict";
import { writeFileSync, realpathSync } from "node:fs";
import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

export function runFixture(configuration: unknown): void {
  const input = z.object({ identity: z.string(), version: z.string() }).parse(configuration);
  assert.deepEqual(process.argv.slice(2), ["--version"]);
  const executable = z.string().parse(process.argv[1]);
  const receipt = {
    invoked_executable: realpathSync(executable),
    identity: input.identity,
    version: input.version,
  };
  writeFileSync(
    z.string().parse(process.env.GLOBAL_HARNESS_FIXTURE_NATIVE_RECEIPT),
    wireJson(receipt),
  );
  console.log(input.version);
}
