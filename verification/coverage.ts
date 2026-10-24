import { execFileSync } from "node:child_process";
import type { Declaration } from "./declaration.ts";
import type { RowOutcome } from "./receipt.ts";

export interface CaseRegistration {
  readonly id: string;
  readonly rows: readonly string[];
  readonly evidence: "journey" | "tokioComponent" | "workerdComponent" | "rustEndToEnd";
}
export type CaseOutcome =
  | { readonly kind: "observed" }
  | { readonly kind: "failed"; readonly reason: string }
  | { readonly kind: "open"; readonly reason: string };
const jointRows = new Set(["OB4:5", "OB4:6", "OB4:7", "OB4:12", "OB4:13", "OB4:16"]);
const jointEvidence: readonly CaseRegistration["evidence"][] = [
  "journey",
  "tokioComponent",
  "workerdComponent",
  "rustEndToEnd",
];
export interface CoverageRow {
  readonly id: string;
  readonly reference: string;
  readonly behavior: string;
}
export function readRows(source: Declaration["vd"]): ReadonlyMap<string, CoverageRow> {
  const document = execFileSync("git", ["show", `${source.revision}:${source.path}`], {
    cwd: source.repository,
    encoding: "utf8",
    timeout: 10_000,
  });
  const start = document.indexOf("**The V10 and OB4 coverage map**");
  const end = document.indexOf("Named limits,", start);
  if (start < 0 || end < start) throw new Error("VD OB4 map not found");
  const rows = new Map<string, CoverageRow>();
  for (const match of document
    .slice(start, end)
    .matchAll(/^\s*\|\s*(\d+|F2|F|W|L|D|M)\s*\|\s*([^|]+)\|/gm)) {
    const id = match[1];
    const behavior = match[2];
    if (id === undefined || behavior === undefined) throw new Error("VD map row malformed");
    if (rows.has(`OB4:${id}`)) throw new Error(`duplicate VD row: ${id}`);
    rows.set(`OB4:${id}`, {
      id: `OB4:${id}`,
      reference: `VD OB4 ${id}`,
      behavior: behavior.trim(),
    });
  }
  for (const id of ["OB5", "OB7"]) {
    if (!document.includes(`**${id}:`)) throw new Error(`VD ${id} missing`);
    rows.set(id, {
      id,
      reference: `VD ${id}`,
      behavior: id === "OB5" ? "upgrade and import rehearsal" : "all four deployed-central groups",
    });
  }
  if (rows.size < 20) throw new Error("VD map unexpectedly incomplete");
  return rows;
}
export function resolveCoverage(
  rows: ReadonlyMap<string, CoverageRow>,
  selected: readonly string[],
  cases: readonly CaseRegistration[],
): ReadonlyMap<string, readonly string[]> {
  if (new Set(cases.map((entry): string => entry.id)).size !== cases.length)
    throw new Error("duplicate case registration");
  for (const entry of cases)
    for (const row of entry.rows)
      if (!rows.has(row)) throw new Error(`case ${entry.id} names absent VD row ${row}`);
  return new Map(
    selected.map((row): [string, readonly string[]] => {
      if (!rows.has(row)) throw new Error(`selected VD row absent: ${row}`);
      return [
        row,
        cases.filter((entry): boolean => entry.rows.includes(row)).map((entry): string => entry.id),
      ];
    }),
  );
}
export function exitCode(outcomes: ReadonlyMap<string, RowOutcome>): 0 | 1 | 2 {
  if (outcomes.size === 0) return 2;
  if ([...outcomes.values()].some((outcome): boolean => outcome.kind === "failed")) return 1;
  if ([...outcomes.values()].some((outcome): boolean => outcome.kind === "open")) return 2;
  return 0;
}

export function collectOutcomes(input: {
  rows: ReadonlyMap<string, CoverageRow>;
  selected: readonly string[];
  cases: readonly CaseRegistration[];
  results: ReadonlyMap<string, CaseOutcome>;
}): ReadonlyMap<string, RowOutcome> {
  const plan = resolveCoverage(input.rows, input.selected, input.cases);
  for (const id of input.results.keys())
    if (!input.cases.some((entry): boolean => entry.id === id))
      throw new Error(`unregistered case result: ${id}`);
  return new Map(
    [...plan].map(([row, ids]): [string, RowOutcome] => {
      if (ids.length === 0) return [row, { kind: "open", reason: "no case registered" }];
      for (const id of ids) {
        const result = input.results.get(id);
        if (result?.kind === "failed")
          return [row, { kind: "failed", case: id, reason: result.reason }];
      }
      for (const id of ids) {
        const result = input.results.get(id);
        if (result === undefined || result.kind === "open")
          return [row, { kind: "open", reason: result?.reason ?? `case not executed: ${id}` }];
      }
      if (jointRows.has(row) || row === "OB4:F") {
        const kinds = new Set(
          input.cases
            .filter((entry): boolean => ids.includes(entry.id))
            .map((entry) => entry.evidence),
        );
        const required =
          row === "OB4:F"
            ? jointEvidence.filter((kind): boolean => kind !== "rustEndToEnd")
            : jointEvidence;
        const missing = required.filter((kind): boolean => !kinds.has(kind));
        if (missing.length > 0)
          return [row, { kind: "open", reason: `adopted row also requires ${missing.join(", ")}` }];
      }
      return [row, { kind: "observed", cases: ids }];
    }),
  );
}
