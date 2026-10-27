import { spawnSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";

const harnessPackage = "codex-router-quota-reset-test-harness";
const packageSchema = z.object({
  name: z.string().min(1),
  version: z.string().min(1),
  manifest_path: z.string().refine(isAbsolute, "Cargo manifest paths must be absolute"),
});
const metadataSchema = z.object({ packages: z.array(packageSchema) });
type CargoPackage = z.infer<typeof packageSchema>;

export interface QuotaInputPaths {
  readonly directories: readonly string[];
  readonly files: readonly string[];
}

export const quotaSupportInputs: QuotaInputPaths = {
  // capture_tools.rs embeds and executes these adapters, with their package dependencies.
  directories: ["scripts/verification/tui", ".cargo"],
  files: [
    // Workspace build inputs and nextest's test profile.
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".config/nextest.toml",
    // collaboration-protocol/src/acp_schema_catalog.rs embeds this schema.
    "protocol-schemas/acp-protocol/stable-schema.json",
    // CI nominates Node and nextest; selection-tool dependency changes need a full run too.
    ".github/workflows/ci.yml",
    ".github/actions/install-ci-tools/action.yml",
    "scripts/tooling/bootstrap-tools.sh",
    "scripts/tooling/tool-versions.tsv",
    "verification/package.json",
    "verification/pnpm-lock.yaml",
  ],
};

function repositoryRelativePath(repositoryRoot: string, path: string): string | undefined {
  const local = relative(repositoryRoot, path);
  if (local === ".." || local.startsWith(`..${sep}`) || isAbsolute(local)) return undefined;
  return local.split(sep).join("/");
}

export function deriveQuotaPackageDirectories(
  rawMetadata: unknown,
  treeOutput: string,
  repositoryRoot: string,
): readonly string[] {
  const metadata = metadataSchema.parse(rawMetadata);
  const byIdentity = new Map<string, CargoPackage[]>();
  for (const item of metadata.packages) {
    const identity = `${item.name} v${item.version}`;
    const packages = byIdentity.get(identity) ?? [];
    packages.push(item);
    byIdentity.set(identity, packages);
  }
  const directories = new Set<string>();
  let harnessSelected = false;
  // Metadata describes the workspace; cargo tree -p selects this harness's active feature graph.
  for (const line of treeOutput.split(/\r?\n/)) {
    const identity = /^(\S+ v\S+)(?:\s|$)/.exec(line)?.[1];
    if (identity === undefined) continue;
    const packages = byIdentity.get(identity) ?? [];
    let selected = packages.length === 1 ? packages[0] : undefined;
    if (packages.length > 1) {
      const matching = packages.filter((item) => line.includes(`(${dirname(item.manifest_path)})`));
      selected = matching.length === 1 ? matching[0] : undefined;
      if (
        selected === undefined &&
        packages.some(
          (item) =>
            repositoryRelativePath(repositoryRoot, dirname(item.manifest_path)) !== undefined,
        )
      )
        throw new Error(`Cargo tree has an ambiguous package: ${identity}`);
    }
    if (selected === undefined) continue;
    const directory = repositoryRelativePath(repositoryRoot, dirname(selected.manifest_path));
    if (directory === undefined) continue;
    directories.add(directory);
    if (selected.name === harnessPackage) harnessSelected = true;
  }
  if (!harnessSelected) throw new Error("The quota harness is absent from Cargo's selected graph");
  return [...directories].sort();
}

export function isQuotaInputPath(path: string, inputs: QuotaInputPaths): boolean {
  return (
    inputs.files.includes(path) ||
    inputs.directories.some(
      (directory) => directory === "" || path === directory || path.startsWith(`${directory}/`),
    )
  );
}

function runTool(executable: string, arguments_: readonly string[], directory: string): string {
  const result = spawnSync(executable, arguments_, {
    cwd: directory,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0)
    throw new Error(`${executable} ${arguments_[0] ?? ""} failed: ${result.stderr.trim()}`);
  return result.stdout;
}

function loadQuotaInputPaths(repositoryRoot: string): QuotaInputPaths {
  const target = /^host: (.+)$/m.exec(runTool("rustc", ["-vV"], repositoryRoot))?.[1];
  if (target === undefined) throw new Error("Cannot determine the native Rust target");
  const rawMetadata: unknown = JSON.parse(
    runTool(
      "cargo",
      ["metadata", "--locked", "--offline", "--format-version", "1"],
      repositoryRoot,
    ),
  );
  const tree = runTool(
    "cargo",
    [
      "tree",
      "--locked",
      "--offline",
      "-p",
      harnessPackage,
      "--target",
      target,
      "--edges",
      "normal,build,dev",
      "--prefix",
      "none",
      "--format",
      "{p}",
      "--color",
      "never",
    ],
    repositoryRoot,
  );
  const selectorPath = repositoryRelativePath(repositoryRoot, fileURLToPath(import.meta.url));
  if (selectorPath === undefined) throw new Error("Quota selector must be inside the repository");
  return {
    directories: [
      ...deriveQuotaPackageDirectories(rawMetadata, tree, repositoryRoot),
      ...quotaSupportInputs.directories,
    ],
    files: [...quotaSupportInputs.files, selectorPath],
  };
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  if (arguments_.length !== 0) {
    console.error("Usage: node verification/tooling/select-quota-stage.ts");
    return 2;
  }
  try {
    const environment = z
      .object({
        QUOTA_DIFF_BASE: z.string().min(1),
        QUOTA_DIFF_HEAD: z.string().min(1),
        GITHUB_OUTPUT: z.string().min(1),
      })
      .parse(process.env);
    const repositoryRoot = runTool(
      "git",
      ["rev-parse", "--show-toplevel"],
      process.cwd(),
    ).trimEnd();
    const inputs = loadQuotaInputPaths(repositoryRoot);
    // Disabling rename detection retains the old path when code moves out of a quota dependency.
    const paths = runTool(
      "git",
      [
        "diff",
        "--name-only",
        "--no-renames",
        "-z",
        `${environment.QUOTA_DIFF_BASE}...${environment.QUOTA_DIFF_HEAD}`,
        "--",
      ],
      repositoryRoot,
    )
      .split("\0")
      .filter(Boolean);
    const matched = paths.filter((path) => isQuotaInputPath(path, inputs));
    console.log(JSON.stringify({ ...inputs, matched }, null, 2));
    appendFileSync(environment.GITHUB_OUTPUT, `changed=${matched.length > 0}\n`);
    return 0;
  } catch (error) {
    const message = (error instanceof Error ? error.message : String(error))
      .replaceAll(process.cwd(), "<workspace>")
      .replaceAll(homedir(), "<home>");
    console.error(`Quota stage selection failed: ${message}`);
    return 2;
  }
}

if (import.meta.main) process.exitCode = main();
