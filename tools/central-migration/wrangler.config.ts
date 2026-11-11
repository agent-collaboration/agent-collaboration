// Tooling for `cf deploy`'s build step: upload build/ as tsc emitted it, with
// no bundling, so the uploaded modules are the tested ones.
export default {
  noBundle: true,
  rules: [{ type: "ESModule", globs: ["**/*.js"], fallthrough: false }],
};
