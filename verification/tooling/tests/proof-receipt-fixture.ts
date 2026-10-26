import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { z } from "zod";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";

export const proofCaseSchema = z.object({
  name: z.string(),
  operation: z.string(),
  arguments: z.array(z.string()),
  originalArguments: z.array(z.string()),
  files: z.record(z.string(), z.string()),
  commands: z.record(z.string(), z.string()),
  observed: z.array(z.string()),
});
export type ProofCase = z.infer<typeof proofCaseSchema>;
export function runProofReceiptCase(input: ProofCase): unknown {
  const root = createFixtureDirectory("proof-receipt");
  const map = (value: string): string => value.replaceAll("<ROOT>", root);
  try {
    for (const [name, content] of Object.entries(input.files)) {
      const path = join(root, name);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, map(content), { mode: name.endsWith(".py") ? 0o700 : 0o600 });
    }
    const bin = join(root, "bin");
    mkdirSync(bin);
    for (const [name, content] of Object.entries(input.commands))
      writeFileSync(join(bin, name), map(content), { mode: 0o700 });
    const result = spawnSync(
      process.execPath,
      [
        new URL("../proof-matrix-receipt.ts", import.meta.url).pathname,
        input.operation,
        ...input.arguments.map(map),
      ],
      {
        cwd: root,
        env: { ...process.env, PATH: `${bin}:${process.env.PATH ?? ""}` },
        encoding: "utf8",
        timeout: 5000,
      },
    );
    if (result.error !== undefined) throw result.error;
    const files = Object.fromEntries(
      input.observed.map((name) => [
        name,
        existsSync(join(root, name))
          ? readFileSync(join(root, name), "utf8").replaceAll(root, "<ROOT>")
          : null,
      ]),
    );
    return {
      exit: result.status,
      stdout: result.stdout.replaceAll(root, "<ROOT>"),
      stderr: result.stderr.replaceAll(root, "<ROOT>"),
      files,
    };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
