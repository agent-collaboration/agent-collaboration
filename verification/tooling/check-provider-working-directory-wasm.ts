import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { z } from "zod";

const defaultArtifact =
  "target/wasm32-unknown-unknown/debug/examples/provider_working_directory_contract.wasm";

function main(arguments_: readonly string[] = process.argv.slice(2)): void {
  const paths = z.array(z.string().min(1)).max(1).parse(arguments_);
  const artifact = paths[0] ?? defaultArtifact;
  const bytes = readFileSync(artifact);
  const module = new WebAssembly.Module(bytes);
  const imports: WebAssembly.Imports = {};
  const importCalls: WebAssembly.ModuleImportDescriptor[] = [];
  for (const entry of WebAssembly.Module.imports(module)) {
    assert.equal(entry.kind, "function", "the contract has only unused function imports");
    const namespace = imports[entry.module] ?? {};
    namespace[entry.name] = (): never => {
      importCalls.push(entry);
      throw new Error(`The pure path contract called an unexpected import: ${entry.name}`);
    };
    imports[entry.module] = namespace;
  }
  const instance = new WebAssembly.Instance(module, imports);
  const entry = instance.exports["main"];
  assert.equal(typeof entry, "function", "the artifact must export the Rust contract entry");
  if (typeof entry !== "function") throw new Error("Rust contract entry is unavailable");
  const rawStatus: unknown = entry(0, 0);
  const status = z.number().int().parse(rawStatus);
  console.log(
    JSON.stringify({
      artifact,
      wasmSHA256: createHash("sha256").update(bytes).digest("hex"),
      mainStatus: status,
      importCalls,
    }),
  );
  assert.deepEqual(importCalls, [], "no substitute may handle path validation");
  assert.equal(status, 0, "the actual wasm contract must enforce lexical Unix working directories");
}

if (import.meta.main) main();
