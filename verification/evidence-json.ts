import { decode, encode } from "./exact-json.ts";

export function decodeEvidenceJson(value: string | Buffer): unknown {
  return decode(
    typeof value === "string" ? value : new TextDecoder("utf-8", { fatal: true }).decode(value),
  );
}

export function evidenceJson(
  value: unknown,
  sorted: boolean = false,
  ensureAscii: boolean = true,
): string {
  if (Array.isArray(value))
    return `[${value.map((entry: unknown) => evidenceJson(entry, sorted, ensureAscii)).join(", ")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    if (sorted) entries.sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
    return `{${entries.map(([key, entry]) => `${evidenceJson(key)}: ${evidenceJson(entry, sorted)}`).join(", ")}}`;
  }
  const text = encode(value);
  return ensureAscii
    ? text.replace(
        /[\u007f-\uffff]/g,
        (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`,
      )
    : text;
}

export function prettyEvidenceJson(
  value: unknown,
  sorted: boolean = false,
  ensureAscii: boolean = true,
): string {
  function render(item: unknown, level: number): string {
    const padding = "  ".repeat(level + 1);
    const closingPadding = "  ".repeat(level);
    if (Array.isArray(item))
      return item.length === 0
        ? "[]"
        : `[\n${item.map((entry: unknown) => `${padding}${render(entry, level + 1)}`).join(",\n")}\n${closingPadding}]`;
    if (item !== null && typeof item === "object") {
      const entries = Object.entries(item);
      if (sorted) entries.sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0));
      return entries.length === 0
        ? "{}"
        : `{\n${entries.map(([key, entry]) => `${padding}${evidenceJson(key, false, ensureAscii)}: ${render(entry, level + 1)}`).join(",\n")}\n${closingPadding}}`;
    }
    return evidenceJson(item, false, ensureAscii);
  }
  return render(value, 0);
}
