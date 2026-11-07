import { spawnSync } from "node:child_process";
import { openSync, closeSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const expectedHead = process.argv[2];
if (!expectedHead) throw new Error("expected pushed head required");
const evidence = resolve("tmp/2026-11-06-central-dpop-prototype/sunclaw");
const tools = resolve("tmp/2026-11-06-central-dpop-prototype/tools");
mkdirSync(evidence, { recursive: true });
mkdirSync(tools, { recursive: true });
const env = {
  ...process.env,
  PATH: `/opt/homebrew/opt/rustup/bin:${process.env.PATH ?? ""}`,
  CARGO_BUILD_JOBS: "4",
};
const actualHead = spawnSync("git", ["rev-parse", "HEAD"], { encoding: "utf8", env }).stdout.trim();
if (actualHead !== expectedHead) throw new Error(`head mismatch ${actualHead} != ${expectedHead}`);
const steps: { name: string; command: string; args: readonly string[]; status: number | null }[] =
  [];
function run(name: string, command: string, args: readonly string[]): void {
  const log = openSync(resolve(evidence, `${name}.log`), "w");
  const result = spawnSync(command, args, { env, stdio: ["ignore", log, log] });
  closeSync(log);
  steps.push({ name, command, args, status: result.status });
  writeFileSync(
    resolve(evidence, "steps.json"),
    `${JSON.stringify({ source_head: actualHead, steps }, null, 2)}\n`,
  );
  if (result.status !== 0)
    throw new Error(`${name} failed: ${result.status}; ${result.error ?? "see log"}`);
}
run("rust-toolchain", "rustup", ["show"]);
run("install-node", "npm", [
  "install",
  "--prefix",
  `${tools}/node`,
  "--no-save",
  "node@26.11.1",
  "pnpm@12.14.0",
]);
env.PATH = `${tools}/node/node_modules/node/bin:${tools}/node/node_modules/.bin:${tools}/bin:${env.PATH}`;
run("node-version", "node", ["--version"]);
run("install-verification", "pnpm", ["--dir", "verification", "install", "--frozen-lockfile"]);
run("fmt", "cargo", ["fmt", "--all", "--", "--check"]);
run("native-clippy", "cargo", [
  "clippy",
  "--locked",
  "-p",
  "collaboration-central-core",
  "-p",
  "collaboration-central-tokio",
  "--all-targets",
  "--",
  "-D",
  "warnings",
]);
run("native-vectors", "cargo", [
  "nextest",
  "run",
  "--locked",
  "-p",
  "collaboration-central-tokio",
  "--test",
  "integration",
  "-E",
  "test(person_dpop::)",
]);
run("install-worker-build", "cargo", [
  "install",
  "worker-build",
  "--version",
  "0.8.7",
  "--locked",
  "--root",
  tools,
  "--target-dir",
  resolve("target"),
]);
run("worker-build", "worker-build", [
  "crates/collaboration-central-cloudflare",
  "--profile",
  "worker-release",
  "--no-opt",
  "--",
  "--locked",
  "--features",
  "conformance",
]);
run("worker-clippy", "cargo", [
  "clippy",
  "--locked",
  "-p",
  "collaboration-central-cloudflare",
  "--target",
  "wasm32-unknown-unknown",
  "--features",
  "conformance",
  "--",
  "-D",
  "warnings",
]);
run("verification-check", "pnpm", ["--dir", "verification", "run", "check"]);
run("verification-lint", "pnpm", ["--dir", "verification", "run", "lint"]);
run("verification-format", "pnpm", ["--dir", "verification", "run", "format:check"]);
run("verification-tests", "pnpm", ["--dir", "verification", "test"]);
run("workerd-probe", "node", [
  "verification/tooling/measure-person-dpop.ts",
  resolve(evidence, "workerd-measurement.json"),
]);
run("module-cycle-ratchet", "cargo", [
  "nextest",
  "run",
  "--locked",
  "-p",
  "workspace-architecture-check",
  "--run-ignored",
  "only",
]);
run("native-scope-suite", "cargo", [
  "nextest",
  "run",
  "--locked",
  "-p",
  "collaboration-central-core",
  "-p",
  "collaboration-central-tokio",
]);
writeFileSync(
  resolve(evidence, "complete.json"),
  JSON.stringify({ source_head: actualHead, steps: steps.length }),
);
