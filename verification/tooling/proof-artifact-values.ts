import { readFileSync, writeFileSync, copyFileSync, globSync } from "node:fs";
import { dirname, join, relative, isAbsolute } from "node:path";
import { z } from "zod";
import { decodeEvidenceJson, prettyEvidenceJson } from "../evidence-json.ts";
import { sourceShellWords } from "../fixtures/source-shell-words.ts";
import { canonicalPath } from "../filesystem-paths.ts";
import { runProcess, type CommandRunner } from "./process-command.ts";

export type ProofRecord = Record<string, unknown>;
export function record(value: unknown): ProofRecord {
  return z.record(z.string(), z.unknown()).parse(value);
}
export function section(value: ProofRecord, name: string): ProofRecord {
  return record(value[name] === undefined ? {} : value[name]);
}
export function readProof(path: string): ProofRecord {
  return record(decodeEvidenceJson(readFileSync(path)));
}
export function writeProof(path: string, value: unknown): void {
  writeFileSync(path, `${prettyEvidenceJson(value, true)}\n`);
}
export function truthy(value: unknown): boolean {
  if (Array.isArray(value)) return value.length !== 0;
  if (value !== null && typeof value === "object") return Object.keys(value).length !== 0;
  return Boolean(value);
}
export function atLeast(value: unknown, minimum: number): boolean {
  if (value === undefined) return 0 >= minimum;
  return z.union([z.number(), z.bigint(), z.boolean().transform(Number)]).parse(value) >= minimum;
}
export function equalNumber(value: unknown, expected: number): boolean {
  return (
    value === expected ||
    value === BigInt(expected) ||
    (typeof value === "boolean" && Number(value) === expected)
  );
}
export function integer(value: unknown): boolean {
  return (
    typeof value === "bigint" ||
    typeof value === "boolean" ||
    (typeof value === "number" && Number.isInteger(value))
  );
}
export function list(value: unknown): unknown[] {
  return value === undefined ? [] : z.array(z.unknown()).parse(value);
}
export function display(value: unknown): string {
  if (value === undefined || value === null) return "None";
  if (value === true) return "True";
  if (value === false) return "False";
  if (typeof value === "string") {
    const quote = value.includes("'") && !value.includes('"') ? '"' : "'";
    return (
      quote +
      value
        .replaceAll("\\", "\\\\")
        .replaceAll(quote, `\\${quote}`)
        .replaceAll("\n", "\\n")
        .replaceAll("\r", "\\r")
        .replaceAll("\t", "\\t") +
      quote
    );
  }
  if (Array.isArray(value)) return `[${value.map(display).join(", ")}]`;
  if (typeof value === "object")
    return `{${Object.entries(value)
      .map(([key, item]) => `${display(key)}: ${display(item)}`)
      .join(", ")}}`;
  return String(value);
}
export function displayText(value: unknown): string {
  return typeof value === "string" ? value : display(value);
}
export function sourcePaths(guard: string): string[] {
  return [
    ...new Set(
      sourceShellWords(guard).flatMap((pattern) => {
        const matches = globSync(pattern).sort();
        return matches.length === 0 ? [pattern] : matches;
      }),
    ),
  ].sort();
}
export function guardedPathsDirty(
  paths: readonly string[],
  runner: CommandRunner = runProcess,
): boolean {
  const worktree = runner(["git", "diff", "--quiet", "HEAD", "--", ...paths], { inherit: true });
  const index = runner(["git", "diff", "--cached", "--quiet", "--", ...paths], { inherit: true });
  return worktree.status !== 0 || index.status !== 0;
}
export function copyArtifact(source: string, receipt: string, name: string): string {
  const destination = join(dirname(receipt), name);
  copyFileSync(source, destination);
  const path = relative(canonicalPath(process.cwd()), canonicalPath(destination));
  return path === ".." || path.startsWith("../") || isAbsolute(path) ? destination : path;
}
export function settleReceipt(
  path: string,
  receipt: ProofRecord,
  errors: readonly string[],
): number {
  Object.assign(receipt, {
    status_after: errors.length === 0 ? "[x] passed" : "[ ] pending",
    result: errors.length === 0 ? "pass" : "fail",
    exit_code: errors.length === 0 ? 0 : 1,
  });
  writeProof(path, receipt);
  for (const error of errors) console.error(error);
  return errors.length === 0 ? 0 : 1;
}
