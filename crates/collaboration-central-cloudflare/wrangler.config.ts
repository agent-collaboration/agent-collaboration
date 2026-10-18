// Tooling for `cf deploy`'s build step: upload the guarded worker-build output
// as built. No bundling; the wasm module the entry imports is carried as a
// compiled module, so the uploaded files are byte-identical to build/.
export default {
  noBundle: true,
  rules: [{ type: "CompiledWasm", globs: ["**/*.wasm"], fallthrough: false }],
};
