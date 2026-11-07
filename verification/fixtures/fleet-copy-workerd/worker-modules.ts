import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { resolve } from "node:path";

// Pinned bindgen/workers-rs output. A changed snippet hash needs review.
export const declaredModulePaths = [
  "shim.mjs",
  "index_bg.js",
  "index_bg.wasm",
  "index.js",
  "snippets/worker-64cc4b2d035528c7/inline0.js",
] as const;
interface WorkerModule {
  type: "ESModule" | "CompiledWasm";
  path: string;
}
function generatedModulePaths(directory: string, prefix = ""): string[] {
  return readdirSync(resolve(directory, prefix), { withFileTypes: true }).flatMap((entry) => {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) return generatedModulePaths(directory, path);
    return /\.(?:mjs|js|wasm)$/.test(entry.name) ? [path] : [];
  });
}
export function checkedWorkerModules(directory: string): WorkerModule[] {
  assert.deepEqual(
    generatedModulePaths(directory).sort(),
    [...declaredModulePaths].sort(),
    "generated runtime modules differ from the reviewed manifest",
  );
  return declaredModulePaths.map((path) => ({
    type: path.endsWith(".wasm") ? "CompiledWasm" : "ESModule",
    path: resolve(directory, path),
  }));
}
