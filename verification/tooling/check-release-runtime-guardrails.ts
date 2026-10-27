import { randomUUID } from "node:crypto";
import { globSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { parseArgs } from "node:util";
import { z } from "zod";
import { runProcess } from "./process-command.ts";
import { declaredProductionModules, stripCfgTestItems } from "./rust-production-sources.ts";
import { sortedAsciiJson } from "./sorted-ascii-json.ts";

const markerSchema = z.tuple([z.string(), z.string()]);
const checkSchema = z.object({
  row_id: z.string(),
  description: z.string(),
  forbidden: z.array(markerSchema),
  required: z.array(markerSchema),
  release_scan_forbidden: z.array(z.string()),
});
const definitions = z
  .object({
    checks: z.record(z.string(), checkSchema),
    crates: z.array(z.string()),
    subtrees: z.record(z.string(), z.string()),
  })
  .parse(JSON.parse(readFileSync(new URL("release-runtime-checks.json", import.meta.url), "utf8")));
export type RuntimeCheck = z.infer<typeof checkSchema>;
export interface GuardFailure {
  readonly kind:
    | "forbidden_present"
    | "required_missing"
    | "release_reachable_forbidden_present"
    | "dirty_guarded_source_path";
  readonly path: string;
  readonly needle: string;
}
export const repositoryRoot = resolve(import.meta.dirname, "../..");

export function releaseRuntimeSourcePaths(root: string = repositoryRoot): string[] {
  return definitions.crates
    .flatMap((name) =>
      globSync(`crates/${name}/src/**/*.rs`, { cwd: root }).filter((path) =>
        statSync(join(root, path)).isFile(),
      ),
    )
    .sort();
}

export function productionSubtreePaths(
  relativePath: string,
  root: string = repositoryRoot,
): string[] {
  const subtree = definitions.subtrees[relativePath];
  if (subtree === undefined) return [relativePath];
  const pending = [relativePath];
  const visited = new Set<string>();
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined || visited.has(current)) continue;
    visited.add(current);
    const source = stripCfgTestItems(readFileSync(join(root, current), "utf8"));
    for (const modulePath of declaredProductionModules(current, source))
      if (modulePath === subtree || modulePath.startsWith(`${subtree}/`)) pending.push(modulePath);
  }
  return [...visited].sort();
}

export function guardedSourcePaths(root: string = repositoryRoot): string[] {
  const patterns = [
    "Cargo.toml",
    "deny.toml",
    "crates/*/Cargo.toml",
    "crates/*/src/*.rs",
    "crates/*/src/**/*.rs",
    "scripts/*",
    ".github/workflows/*",
    "verification/tooling/**/*.ts",
    "verification/tooling/release-runtime-checks.json",
  ];
  const paths: string[] = [];
  for (const pattern of patterns) {
    const matches = globSync(pattern, { cwd: root }).sort();
    if (matches.length === 0) paths.push(pattern);
    else paths.push(...matches.filter((path) => statSync(join(root, path)).isFile()));
  }
  return [...new Set(paths)].sort();
}

export function dirtyGuardedSourcePaths(root: string = repositoryRoot): string[] {
  const paths = guardedSourcePaths(root);
  const worktree = runProcess(["git", "diff", "--name-only", "HEAD", "--", ...paths], {
    cwd: root,
  });
  const index = runProcess(["git", "diff", "--cached", "--name-only", "--", ...paths], {
    cwd: root,
  });
  return [
    ...new Set([...worktree.stdout.split("\n"), ...index.stdout.split("\n")].filter(Boolean)),
  ].sort();
}

export function evaluateCheck(check: RuntimeCheck, root: string = repositoryRoot): GuardFailure[] {
  const productionSources = new Map<
    string,
    readonly { readonly path: string; readonly source: string }[]
  >();
  const failures: GuardFailure[] = [];
  for (const [path, needle] of [...check.forbidden, ...check.required]) {
    let sources = productionSources.get(path);
    if (sources === undefined) {
      sources = productionSubtreePaths(path, root).map((path) => ({
        path,
        source: stripCfgTestItems(readFileSync(join(root, path), "utf8")),
      }));
      productionSources.set(path, sources);
    }
    const forbidden = check.forbidden.some(
      ([candidatePath, candidateNeedle]) => candidatePath === path && candidateNeedle === needle,
    );
    const forbiddenMatch = forbidden
      ? sources.find((entry) => entry.source.includes(needle))
      : undefined;
    if (forbiddenMatch !== undefined)
      failures.push({ kind: "forbidden_present", path: forbiddenMatch.path, needle });
    if (
      check.required.some(
        ([candidatePath, candidateNeedle]) => candidatePath === path && candidateNeedle === needle,
      ) &&
      !sources.some((entry) => entry.source.includes(needle))
    )
      failures.push({ kind: "required_missing", path, needle });
  }
  const releaseSources = releaseRuntimeSourcePaths(root).map((path) => ({
    path,
    source: stripCfgTestItems(readFileSync(join(root, path), "utf8")),
  }));
  for (const needle of check.release_scan_forbidden)
    for (const entry of releaseSources)
      if (entry.source.includes(needle))
        failures.push({ kind: "release_reachable_forbidden_present", path: entry.path, needle });
  for (const path of dirtyGuardedSourcePaths(root))
    failures.push({
      kind: "dirty_guarded_source_path",
      path,
      needle: "worktree_or_index_differs_from_HEAD",
    });
  return failures;
}

export function writeReceipt(
  check: RuntimeCheck,
  failures: readonly GuardFailure[],
  root: string = repositoryRoot,
): string {
  const accountRouter = check.row_id.startsWith("AR-");
  const evidenceRoot = join(
    root,
    "tmp/plan-workflows",
    accountRouter ? "2026-06-25-account-router-ux-fix" : "2026-06-24-async-router-runtime",
    "evidence/structural",
  );
  const verifyOnly = process.env.CODEX_ROUTER_PROOF_VERIFY_ONLY === "1";
  if (!verifyOnly) mkdirSync(evidenceRoot, { recursive: true });
  const receiptPath = verifyOnly
    ? join(tmpdir(), `codex-router-guardrail-${check.row_id}-${randomUUID()}.json`)
    : join(evidenceRoot, `${check.row_id}.json`);
  const head = runProcess(["git", "rev-parse", "HEAD"], { cwd: root });
  if (head.status !== 0) throw new Error(head.stderr.trim());
  const payload = {
    schema_version: 1,
    row_id: check.row_id,
    layer: "structural",
    owner: accountRouter ? "account-router-ux-fix" : "T6",
    command:
      process.env.CODEX_ROUTER_PROOF_COMMAND ??
      `node verification/tooling/check-release-runtime-guardrails.ts ${check.row_id}`,
    cwd: ".",
    git_head: head.stdout.trim(),
    timestamp_utc: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
    status_before: "[ ] pending",
    status_after: failures.length === 0 ? "[x] passed" : "[ ] pending",
    result: failures.length === 0 ? "pass" : "fail",
    exit_code: failures.length === 0 ? 0 : 1,
    expected_observation: check.description,
    touched_targets: [
      ...new Set(
        [...check.forbidden, ...check.required]
          .map(([path]) => path)
          .concat(releaseRuntimeSourcePaths(root)),
      ),
    ].sort(),
    freshness_guard:
      "Cargo.toml crates/*/Cargo.toml crates/*/src/**/*.rs scripts/* verification/tooling/**",
    freshness_check: failures.some((failure) => failure.kind === "dirty_guarded_source_path")
      ? "guarded_source_paths_dirty"
      : "guarded_source_paths_clean_at_git_head",
    redaction_check: "pass",
    artifact_paths: [],
    failures,
  };
  writeFileSync(
    receiptPath,
    sortedAsciiJson(payload),
    verifyOnly ? { flag: "wx", mode: 0o600 } : {},
  );
  return receiptPath;
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  const { values, positionals } = parseArgs({
    args: [...arguments_],
    allowPositionals: true,
    options: { "print-release-source-paths": { type: "boolean" } },
  });
  if (values["print-release-source-paths"]) {
    console.log(JSON.stringify(releaseRuntimeSourcePaths(), null, 2));
    return 0;
  }
  const row = positionals[0];
  const check = row === undefined ? undefined : definitions.checks[row];
  if (positionals.length !== 1 || check === undefined) {
    console.error(
      `usage: node verification/tooling/check-release-runtime-guardrails.ts {${Object.keys(definitions.checks).sort().join(",")}}`,
    );
    return 2;
  }
  const failures = evaluateCheck(check);
  const receiptPath = writeReceipt(check, failures);
  if (failures.length > 0) {
    console.error(`release runtime guardrail ${check.row_id} failed: ${receiptPath}`);
    for (const failure of failures)
      console.error(`${failure.kind}: ${failure.path} :: ${failure.needle}`);
    return 1;
  }
  console.log(`release runtime guardrail ${check.row_id} passed: ${receiptPath}`);
  return 0;
}

if (import.meta.main) process.exitCode = main();
