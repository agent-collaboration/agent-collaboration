import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { encode } from "../exact-json.ts";

export const nativeCaseSchema = z.object({
  name: z.string(),
  programs: z.record(z.string(), z.object({ module: z.string(), configuration: z.unknown() })),
  executable: z.string(),
  arguments: z.array(z.string()),
  input: z.string(),
  environment: z.record(z.string(), z.string()),
  initialFiles: z.record(z.string(), z.string()),
});
export type NativeCase = z.infer<typeof nativeCaseSchema>;
export function mapFixtureRoot(value: unknown, root: string): unknown {
  if (typeof value === "string") return value.replaceAll("<ROOT>", root);
  if (Array.isArray(value)) return value.map((item: unknown) => mapFixtureRoot(item, root));
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, mapFixtureRoot(item, root)]),
    );
  return value;
}

export function runNativeExecutableFixture(input: NativeCase): unknown {
  const root = createFixtureDirectory("native-wire");
  const entry = nativeCaseSchema.parse(mapFixtureRoot(input, root));
  try {
    for (const [name, content] of Object.entries(entry.initialFiles)) {
      const path = join(root, name);
      mkdirSync(join(path, ".."), { recursive: true });
      writeFileSync(path, content);
    }
    for (const [name, program] of Object.entries(entry.programs)) {
      const file = join(root, name);
      const module = new URL(`./${program.module}`, import.meta.url).pathname;
      writeFileSync(
        file,
        `#!${process.execPath}\nimport { runFixture } from ${JSON.stringify(module)};\nawait runFixture(${encode(program.configuration)});\n`,
        { mode: 0o700 },
      );
    }
    const environment: NodeJS.ProcessEnv = { PATH: root, ...entry.environment };
    const result = spawnSync(join(root, entry.executable), entry.arguments, {
      cwd: root,
      env: environment,
      input: entry.input,
      encoding: "utf8",
      timeout: 5000,
    });
    if (result.error !== undefined) throw result.error;
    const files = Object.fromEntries(
      readdirSync(root)
        .filter((name) => !Object.hasOwn(entry.programs, name))
        .sort()
        .map((name) => [name, readFileSync(join(root, name), "utf8").replaceAll(root, "<ROOT>")]),
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
