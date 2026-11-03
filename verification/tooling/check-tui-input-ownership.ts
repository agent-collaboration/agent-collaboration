import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const workspaceRoot = resolve(import.meta.dirname, "../..");
const outputRoot = resolve(workspaceRoot, "tmp/tui-input-ownership");
const cargoArguments = [
  "clippy",
  "--keep-going",
  "-p",
  "agent-session-tui",
  "-p",
  "agent-router-quota-tui",
  "-p",
  "agent-collaboration-tui",
  "--examples",
  "--",
  "-D",
  "warnings",
];
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

function fixtureName(control: LintControl): string {
  return `s8_input_ownership_${control.name.replaceAll("-", "_")}.rs`;
}

function fixturePath(control: LintControl): string {
  return resolve(workspaceRoot, "crates", control.crateName, "examples", fixtureName(control));
}

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
  }
  for (const control of controls) {
    const controlPath = fixturePath(control);
    if (existsSync(controlPath)) throw new Error(`Refusing to overwrite a fixture: ${controlPath}`);
  }
}

function main(): void {
  if (process.argv.slice(2).some((argument) => argument !== "--check"))
    throw new Error("Usage: node verification/tooling/check-tui-input-ownership.ts [--check]");
  preflight();
  if (process.argv.includes("--check")) {
    console.log("TUI input-ownership controls are ready; no Cargo command or mutation ran.");
    return;
  }
  const installed: { readonly path: string; readonly source: string }[] = [];
  const cleanupErrors: unknown[] = [];
  let output = "";
  let outcome: { readonly kind: "passed" } | { readonly kind: "failed"; readonly error: unknown } =
    {
      kind: "passed",
    };
  try {
    for (const control of controls) {
      const controlPath = fixturePath(control);
      const source = fixtureSource(control);
      mkdirSync(resolve(controlPath, ".."), { recursive: true });
      writeFileSync(controlPath, source, { flag: "wx" });
      installed.push({ path: controlPath, source });
    }
    const result = spawnSync("cargo", cargoArguments, {
      cwd: workspaceRoot,
      env: { ...process.env, CARGO_TERM_COLOR: "never" },
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
    });
    output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    if (result.error !== undefined) throw result.error;
    if (result.signal !== null || result.status === null)
      throw new Error(`Clippy terminated without an exit status (${result.signal})`);
    if (result.status === 0) throw new Error("Clippy accepted the disallowed listener fixtures");
    const diagnostics = output
      .split(/(?=^(?:error|warning)(?:\[[^\]]+\])?:)/m)
      .filter((diagnostic) => /^(?:error|warning)(?:\[[^\]]+\])?:/.test(diagnostic));
    for (const control of controls) {
      const method = control.method;
      const controlName = fixtureName(control);
      if (method === null) {
        if (diagnostics.some((diagnostic) => diagnostic.includes(controlName)))
          throw new Error(`${control.name}: the render-only fixture produced a diagnostic`);
      } else if (
        !diagnostics.some((diagnostic) => {
          const headline = diagnostic.split("\n", 1)[0] ?? "";
          return (
            headline.includes("disallowed method") &&
            headline.includes(method) &&
            diagnostic.includes(controlName)
          );
        })
      ) {
        throw new Error(
          `${control.name}: the named fixture did not produce its disallowed-method diagnostic`,
        );
      }
    }
  } catch (error) {
    outcome = { kind: "failed", error };
  } finally {
    for (const fixture of installed) {
      try {
        if (readFileSync(fixture.path, "utf8") !== fixture.source)
          cleanupErrors.push(
            new Error(`Control changed during execution; retained for inspection: ${fixture.path}`),
          );
        else unlinkSync(fixture.path);
      } catch (error) {
        cleanupErrors.push(error);
      }
    }
  }
  if (cleanupErrors.length > 0)
    outcome = {
      kind: "failed",
      error: new AggregateError(
        outcome.kind === "failed" ? [outcome.error, ...cleanupErrors] : cleanupErrors,
        "TUI control restoration failed",
      ),
    };
  if (outcome.kind === "failed") {
    if (output.length > 0) {
      mkdirSync(outputRoot, { recursive: true });
      writeFileSync(resolve(outputRoot, "clippy.log"), output);
      console.error(`Clippy output: ${outputRoot}/clippy.log`);
    }
    throw outcome.error;
  }
  console.log("Three listener controls rejected; render-only control passed. No fixture remains.");
}

main();
