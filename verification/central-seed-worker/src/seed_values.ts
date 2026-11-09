// The extractor's SqlValue wire, read losslessly: integers never pass through
// a JS number. Shared by the driver, the Worker and the tests.

import type { SeedValue } from "./table_digest.ts";

/** Decodes one wire value parsed by lossless-json (numbers arrive as LosslessNumber-like objects). */
export function seedValue(wire: unknown, where: string): SeedValue {
  if (typeof wire !== "object" || wire === null) throw new Error(`${where}: not a SqlValue`);
  const { kind, value } = wire as { kind?: unknown; value?: unknown };
  const numberText = (raw: unknown): string => {
    if (typeof raw === "object" && raw !== null && "isLosslessNumber" in raw) return String(raw);
    throw new Error(`${where}: ${String(kind)} value must be a JSON number`);
  };
  switch (kind) {
    case "null":
      return { kind: "null" };
    case "integer":
      return { kind: "integer", value: numberText(value) };
    case "real":
      if (value === "positiveInfinity") return { kind: "real", value: Number.POSITIVE_INFINITY };
      if (value === "negativeInfinity") return { kind: "real", value: Number.NEGATIVE_INFINITY };
      return { kind: "real", value: Number(numberText(value)) };
    case "text":
      if (typeof value !== "string") throw new Error(`${where}: text must be a string`);
      return { kind: "text", value };
    case "blob": {
      if (typeof value !== "string") throw new Error(`${where}: blob must be base64`);
      const binary = atob(value);
      return { kind: "blob", value: Uint8Array.from(binary, (char) => char.charCodeAt(0)) };
    }
    default:
      throw new Error(`${where}: unknown SqlValue kind ${String(kind)}`);
  }
}
