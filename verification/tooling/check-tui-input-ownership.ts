import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const workspaceRoot = resolve(import.meta.dirname, "../..");
const outputRoot = resolve(workspaceRoot, "tmp/tui-input-ownership");
const cargoArguments = ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"];
const fixtureName = "s8_input_ownership_control.rs";
const crateNames = ["agent-session-tui", "agent-router-quota-tui", "agent-collaboration-tui"];
const methods = ["use_terminal_events", "use_local_terminal_events", "use_terminal_size"];

interface LintControl {
  readonly name: string;
  readonly crateName: string;
  readonly component: string;
  readonly method: "use_terminal_events" | "use_local_terminal_events" | null;
}

const controls: readonly LintControl[] = [
  {
    name: "consumer-global-listener",
    crateName: "agent-session-tui",
    component: "ConsumerControl",
    method: "use_terminal_events",
  },
  {
    name: "consumer-local-listener",
    crateName: "agent-router-quota-tui",
    component: "ConsumerControl",
    method: "use_local_terminal_events",
  },
  {
    name: "shared-dialog-button-listener",
    crateName: "agent-collaboration-tui",
    component: "DialogButton",
    method: "use_local_terminal_events",
  },
  {
    name: "render-only",
    crateName: "agent-collaboration-tui",
    component: "RenderOnlyControl",
    method: null,
  },
];

function fixtureSource(control: LintControl): string {
  const parameters = control.method === null ? "" : "mut hooks: Hooks";
  const subscription = control.method === null ? "" : `hooks.${control.method}(|_| {});`;
  return `use iocraft::prelude::*;

#[component]
fn ${control.component}(${parameters}) -> impl Into<AnyElement<'static>> {
    ${subscription}
    element!(Text(content: "Input ownership control"))
}

fn main() {
    let _ = element!(${control.component});
}
`;
}

function preflight(): void {
  for (const crateName of crateNames) {
    const configurationPath = resolve(workspaceRoot, "crates", crateName, "clippy.toml");
    if (!existsSync(configurationPath))
      throw new Error(`Consumer integration prerequisite missing: ${configurationPath}`);
    const configuration = readFileSync(configurationPath, "utf8");
    for (const method of methods) {
      if (!configuration.includes(`::${method}"`))
        throw new Error(`${configurationPath} does not disallow ${method}`);
    }
    const fixturePath = resolve(workspaceRoot, "crates", crateName, "examples", fixtureName);
    if (existsSync(fixturePath)) throw new Error(`Refusing to overwrite a fixture: ${fixturePath}`);
  }
}

function runClippy(name: string): { readonly status: number; readonly output: string } {
  const result = spawnSync("cargo", cargoArguments, {
    cwd: workspaceRoot,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  writeFileSync(resolve(outputRoot, `${name}.log`), output);
  if (result.error !== undefined) throw result.error;
  if (result.signal !== null || result.status === null)
    throw new Error(`${name}: Clippy terminated without an exit status (${result.signal})`);
  return { status: result.status, output };
}

function main(): void {
  if (process.argv.slice(2).some((argument) => argument !== "--check"))
    throw new Error("Usage: node verification/tooling/check-tui-input-ownership.ts [--check]");
  preflight();
  if (process.argv.includes("--check")) {
    console.log("TUI input-ownership controls are ready; no Cargo command or mutation ran.");
    return;
  }
  mkdirSync(outputRoot, { recursive: true });
  const receipts: {
    readonly name: string;
    readonly exitCode: number;
    readonly expectedFailure: boolean;
  }[] = [];
  const baseline = runClippy("baseline");
  if (baseline.status !== 0)
    throw new Error(
      `Correct workspace Clippy failed (${baseline.status}); no negative control was installed. See ${outputRoot}/baseline.log`,
    );
  for (const control of controls) {
    const examplesPath = resolve(workspaceRoot, "crates", control.crateName, "examples");
    const fixturePath = resolve(examplesPath, fixtureName);
    mkdirSync(examplesPath, { recursive: true });
    const source = fixtureSource(control);
    writeFileSync(fixturePath, source, { flag: "wx" });
    let outcome: { readonly kind: "passed" } | { readonly kind: "failed"; readonly error: unknown };
    try {
      const result = runClippy(control.name);
      const expectedFailure = control.method !== null;
      if (expectedFailure) {
        if (
          result.status === 0 ||
          !result.output.includes(fixtureName) ||
          !result.output.includes("disallowed method") ||
          !result.output.includes(control.method ?? "")
        ) {
          throw new Error(
            `${control.name}: the named fixture did not produce its disallowed-method diagnostic; see ${outputRoot}/${control.name}.log`,
          );
        }
      } else if (result.status !== 0) {
        throw new Error(
          `Render-only control failed (${result.status}); see ${outputRoot}/${control.name}.log`,
        );
      }
      receipts.push({ name: control.name, exitCode: result.status, expectedFailure });
      outcome = { kind: "passed" };
    } catch (error) {
      outcome = { kind: "failed", error };
    }
    if (readFileSync(fixturePath, "utf8") !== source) {
      const changed = new Error(
        `Control changed during execution; retained for inspection: ${fixturePath}`,
      );
      throw new AggregateError(
        outcome.kind === "failed" ? [outcome.error, changed] : [changed],
        "TUI control restoration failed",
      );
    }
    unlinkSync(fixturePath);
    if (outcome.kind === "failed") throw outcome.error;
  }
  writeFileSync(
    resolve(outputRoot, "receipts.json"),
    `${JSON.stringify({ command: ["cargo", ...cargoArguments], receipts }, null, 2)}\n`,
  );
  console.log("Three listener controls rejected; render-only control passed. No fixture remains.");
}

main();
