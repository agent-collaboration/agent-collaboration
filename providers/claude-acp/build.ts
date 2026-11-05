import { build } from "esbuild";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const zodLicense = await readFile(new URL("./node_modules/zod/LICENSE", import.meta.url), "utf8");
const result = await build({
  banner: { js: `/* Bundled Zod (MIT)\n${zodLicense}\n*/` },
  entryPoints: [fileURLToPath(new URL("./src/router-claude-entry.ts", import.meta.url))],
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  write: false,
  minify: true,
  legalComments: "none",
  sourcemap: false,
});
const output = result.outputFiles[0];
if (!output) throw new Error("Claude metadata build produced no module");
const destination = new URL(
  "../../crates/acp-provider-adapters/src/claude_metadata.mjs",
  import.meta.url,
);
if (process.argv.includes("--check")) {
  if ((await readFile(destination, "utf8")) !== output.text)
    throw new Error("Embedded Claude metadata module is stale; run build");
} else await writeFile(destination, output.text);
