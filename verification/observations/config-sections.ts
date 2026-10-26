import { createHash } from "node:crypto";
import { isAbsolute } from "node:path";
import { parse } from "smol-toml";
import { z } from "zod";
import { canonicalPath } from "../filesystem-paths.ts";

export const sectionFingerprintSchema = z.strictObject({
  header: z.string(),
  table: z.array(z.string()),
  occurrence: z.number().int(),
  sha256: z.string(),
  names_run: z.boolean(),
});
export type SectionFingerprint = z.infer<typeof sectionFingerprintSchema>;
export const sectionChangeSchema = z.strictObject({
  header: z.string(),
  occurrence: z.number().int(),
  before_sha256: z.string().nullable(),
  after_sha256: z.string().nullable(),
  classification: z.enum([
    "externalDesktopSection",
    "externalProjectSection",
    "runReference",
    "unexpected",
  ]),
});
export type SectionChange = z.infer<typeof sectionChangeSchema>;

function quoteAfter(line: string, initialQuote: string | null): string | null {
  let quote = initialQuote;
  let position = 0;
  while (position < line.length) {
    const character = line[position];
    if (quote === null) {
      if (character === "#") break;
      if (character === "'" || character === '"') {
        quote = line.startsWith(character.repeat(3), position) ? character.repeat(3) : character;
        position += quote.length;
        continue;
      }
    } else if (quote.startsWith('"') && character === "\\") {
      position += 2;
      continue;
    } else if (line.startsWith(quote, position)) {
      position += quote.length;
      quote = null;
      continue;
    }
    position++;
  }
  return quote;
}

function tableHeader(line: string): string {
  const stripped = line.trimStart();
  const array = stripped.startsWith("[[");
  const closing = array ? "]]" : "]";
  let quote: string | null = null;
  let position = array ? 2 : 1;
  while (position < stripped.length) {
    const character = stripped[position];
    if (quote === '"' && character === "\\") {
      position += 2;
      continue;
    }
    if (quote !== null) {
      if (character === quote) quote = null;
    } else if (character === "'" || character === '"') quote = character;
    else if (stripped.startsWith(closing, position))
      return stripped.slice(0, position + closing.length);
    position++;
  }
  throw new Error("table header has no closing bracket");
}

function tablePath(header: string): string[] {
  const sentinel = "__verification_section_header__";
  let current: unknown = parse(`${header}\n${sentinel} = true\n`);
  const path: string[] = [];
  for (;;) {
    if (Array.isArray(current)) {
      if (current.length !== 1) break;
      current = current[0];
      continue;
    }
    const parsed = z.record(z.string(), z.unknown()).safeParse(current);
    if (!parsed.success) break;
    const entries = Object.entries(parsed.data);
    if (entries.length !== 1) break;
    const entry = entries[0];
    if (entry === undefined) break;
    const [key, value] = entry;
    if (key === sentinel && value === true) return path;
    path.push(key);
    current = value;
  }
  throw new Error("ambiguous table header");
}

function containsReference(text: string, reference: string): boolean {
  if (!reference.startsWith("/")) return text.includes(reference);
  let start = 0;
  for (;;) {
    const position = text.indexOf(reference, start);
    if (position < 0) return false;
    const ending = position + reference.length;
    const next = text[ending];
    if (next === undefined || /\s/.test(next) || "/\"',]}):;".includes(next)) return true;
    start = ending;
  }
}

function mentions(value: unknown, references: readonly string[]): boolean {
  if (typeof value === "string")
    return references.some((reference) => containsReference(value, reference));
  if (Array.isArray(value)) return value.some((item: unknown) => mentions(item, references));
  const parsed = z.record(z.string(), z.unknown()).safeParse(value);
  return (
    parsed.success &&
    Object.entries(parsed.data).some(
      ([key, item]) => mentions(key, references) || mentions(item, references),
    )
  );
}

function relatedProject(table: readonly string[], references: readonly string[]): boolean {
  const projectPath = table[1];
  if (table[0] !== "projects" || projectPath === undefined || !isAbsolute(projectPath))
    return false;
  const project = canonicalPath(projectPath);
  return references.filter(isAbsolute).some((reference) => {
    const candidate = canonicalPath(reference);
    return (
      project === candidate ||
      candidate.startsWith(project.endsWith("/") ? project : `${project}/`) ||
      project.startsWith(candidate.endsWith("/") ? candidate : `${candidate}/`)
    );
  });
}

function linesWithEndings(text: string): string[] {
  const lines: string[] = [];
  let start = 0;
  for (let index = 0; index < text.length; index++) {
    if (![10, 13, 11, 12, 28, 29, 30, 133, 8232, 8233].includes(text.charCodeAt(index))) continue;
    if (text[index] === "\r" && text[index + 1] === "\n") index++;
    lines.push(text.slice(start, index + 1));
    start = index + 1;
  }
  if (start < text.length) lines.push(text.slice(start));
  return lines;
}

export function fingerprintSections(
  contents: Buffer,
  runReferences: readonly string[],
): SectionFingerprint[] {
  const text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(contents);
  parse(text);
  const references = runReferences.filter(Boolean);
  const sections: { header: string; raw: string }[] = [];
  let header = "<root>";
  let lines: string[] = [];
  let quote: string | null = null;
  for (const line of linesWithEndings(text)) {
    if (quote === null && line.trimStart().startsWith("[")) {
      sections.push({ header, raw: lines.join("") });
      header = tableHeader(line);
      lines = [];
    }
    lines.push(line);
    quote = quoteAfter(line, quote);
  }
  sections.push({ header, raw: lines.join("") });
  const occurrences = new Map<string, number>();
  return sections.map(({ header, raw }): SectionFingerprint => {
    const table = header === "<root>" ? [] : tablePath(header);
    const key = JSON.stringify(table);
    const occurrence = occurrences.get(key) ?? 0;
    occurrences.set(key, occurrence + 1);
    const parsed = parse(raw);
    return {
      header,
      table,
      occurrence,
      sha256: createHash("sha256").update(raw).digest("hex"),
      names_run:
        references.some((reference) => containsReference(raw, reference)) ||
        mentions(parsed, references) ||
        relatedProject(table, references),
    };
  });
}

function tableStartsWith(table: readonly string[], prefix: readonly string[]): boolean {
  return prefix.every((part, index) => table[index] === part);
}
function sectionKey(section: SectionFingerprint): string {
  return JSON.stringify([section.table, section.occurrence]);
}
function compareTables(left: readonly string[], right: readonly string[]): number {
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    const leftPart = left[index];
    const rightPart = right[index];
    if (leftPart !== rightPart && leftPart !== undefined && rightPart !== undefined)
      return leftPart < rightPart ? -1 : 1;
  }
  return left.length - right.length;
}

function arrayElementOwners(sections: readonly SectionFingerprint[]): Map<string, string> {
  const activeArrays = new Map<string, { table: readonly string[]; occurrence: number }>();
  const owners = new Map<string, string>();
  for (const section of sections) {
    if (section.header.startsWith("[[")) {
      for (const [key, active] of activeArrays)
        if (tableStartsWith(active.table, section.table)) activeArrays.delete(key);
      activeArrays.set(JSON.stringify(section.table), {
        table: section.table,
        occurrence: section.occurrence,
      });
    }
    owners.set(
      sectionKey(section),
      JSON.stringify(
        [...activeArrays.values()]
          .sort((left, right) => compareTables(left.table, right.table))
          .filter((active) => tableStartsWith(section.table, active.table))
          .map((active) => [active.table, active.occurrence]),
      ),
    );
  }
  return owners;
}

export function compareSections(
  before: readonly SectionFingerprint[],
  after: readonly SectionFingerprint[],
): SectionChange[] {
  const old = new Map(before.map((section) => [sectionKey(section), section]));
  const next = new Map(after.map((section) => [sectionKey(section), section]));
  const oldOwners = arrayElementOwners(before);
  const newOwners = arrayElementOwners(after);
  const representatives = new Map([...old, ...next]);
  const sorted = [...representatives].sort(
    ([, left], [, right]) =>
      compareTables(left.table, right.table) || left.occurrence - right.occurrence,
  );
  const changes: SectionChange[] = [];
  for (const [key, representative] of sorted) {
    const left = old.get(key);
    const right = next.get(key);
    if (
      left !== undefined &&
      right !== undefined &&
      left.sha256 === right.sha256 &&
      oldOwners.get(key) === newOwners.get(key)
    )
      continue;
    const table = representative.table;
    const classification =
      left?.names_run || right?.names_run
        ? "runReference"
        : table[0] === "desktop"
          ? "externalDesktopSection"
          : table[0] === "projects" && table[1] !== undefined && isAbsolute(table[1])
            ? "externalProjectSection"
            : "unexpected";
    changes.push({
      header: representative.header,
      occurrence: representative.occurrence,
      before_sha256: left?.sha256 ?? null,
      after_sha256: right?.sha256 ?? null,
      classification,
    });
  }
  return changes;
}

export function allowedSectionChange(change: SectionChange): boolean {
  return (
    change.classification === "externalDesktopSection" ||
    change.classification === "externalProjectSection"
  );
}
