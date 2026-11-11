// The sync library's per-table digest, ported exactly: SHA-256 over "[" +
// each row's serde_json encoding joined by "," + "]"
// (fleet-sync-protocol lifecycle_message.rs TableDigestBuilder, identical at
// b9da4b5 and 0b27429). A row is a list of SqlValue in its wire form
// (replication_piece.rs SqlValueWire): {"kind":"null"}, {"kind":"integer",
// "value":i64}, {"kind":"real","value":f64 | "positiveInfinity" |
// "negativeInfinity"}, {"kind":"text","value":s}, {"kind":"blob","value":
// base64}. Integers travel as decimal text here so no i64 passes through a JS
// number; reals are JS numbers, which are exact f64.

import { sha256Hex } from "./sha256_sync.ts";

export type SeedValue =
  | { readonly kind: "null" }
  | { readonly kind: "integer"; readonly value: string }
  | { readonly kind: "real"; readonly value: number }
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "blob"; readonly value: Uint8Array };

const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

/** A canonical i64 decimal: what serde_json writes for an Integer. */
export function canonicalInteger(text: string): string {
  if (!/^-?(0|[1-9]\d*)$/.test(text) || text === "-0") {
    throw new Error(`not a canonical integer: ${text}`);
  }
  const value = BigInt(text);
  if (value < I64_MIN || value > I64_MAX) throw new Error(`integer outside i64: ${text}`);
  return text;
}

/**
 * serde_json's f64 text: the shortest round-trip digits, as decimal while the
 * point falls within 16 places (or 4 leading zeros), otherwise scientific with
 * a signed exponent: "1.0", "0.00001", "1e-6", "1e+16", "1.5e+300", "-0.0".
 * Thresholds observed from serde_json through the Rust fixture test.
 */
export function serdeReal(value: number): string {
  if (Number.isNaN(value)) throw new Error("SQL real must not be NaN");
  if (Object.is(value, -0)) return "-0.0";
  if (value === 0) return "0.0";
  const negative = value < 0;
  // toExponential() with no argument gives the shortest round-trip digits.
  const [mantissa = "", exponentText = "0"] = Math.abs(value).toExponential().split("e");
  const digits = mantissa.replace(".", "");
  const length = digits.length;
  // value = 0.digits × 10^point, i.e. the decimal point sits `point` digits in.
  const point = Number(exponentText) + 1;
  let text: string;
  if (point >= length && point <= 16) {
    text = `${digits}${"0".repeat(point - length)}.0`;
  } else if (point > 0 && point <= 16) {
    text = `${digits.slice(0, point)}.${digits.slice(point)}`;
  } else if (point > -5 && point <= 0) {
    text = `0.${"0".repeat(-point)}${digits}`;
  } else {
    const exponent = point - 1;
    const signed = exponent > 0 ? `+${exponent}` : String(exponent);
    text = length === 1 ? `${digits}e${signed}` : `${digits[0]}.${digits.slice(1)}e${signed}`;
  }
  return negative ? `-${text}` : text;
}

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/** One SqlValue exactly as serde_json serializes it. */
export function serdeValue(value: SeedValue): string {
  switch (value.kind) {
    case "null":
      return '{"kind":"null"}';
    case "integer":
      return `{"kind":"integer","value":${canonicalInteger(value.value)}}`;
    case "real":
      if (value.value === Number.POSITIVE_INFINITY)
        return '{"kind":"real","value":"positiveInfinity"}';
      if (value.value === Number.NEGATIVE_INFINITY)
        return '{"kind":"real","value":"negativeInfinity"}';
      return `{"kind":"real","value":${serdeReal(value.value)}}`;
    case "text":
      return `{"kind":"text","value":${serdeString(value.value)}}`;
    case "blob":
      return `{"kind":"blob","value":${serdeString(base64(value.value))}}`;
  }
}

/** serde_json's string escaping: `"` `\\` and control characters only; no \u escapes for non-ASCII. */
function serdeString(text: string): string {
  let out = '"';
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    if (char === '"') out += '\\"';
    else if (char === "\\") out += "\\\\";
    else if (char === "\n") out += "\\n";
    else if (char === "\r") out += "\\r";
    else if (char === "\t") out += "\\t";
    else if (code === 0x08) out += "\\b";
    else if (code === 0x0c) out += "\\f";
    else if (code < 0x20) out += `\\u${code.toString(16).padStart(4, "0")}`;
    else out += char;
  }
  return `${out}"`;
}

/** The text TableDigestBuilder hashes for `rows`. */
export function digestText(rows: readonly (readonly SeedValue[])[]): string {
  return `[${rows.map((row) => `[${row.map(serdeValue).join(",")}]`).join(",")}]`;
}

/**
 * The table digest for rows in primary-key order, as lowercase hex SHA-256.
 * Synchronous, so it runs inside a Durable Object storage transaction.
 */
export function tableDigest(rows: readonly (readonly SeedValue[])[]): string {
  return sha256Hex(new TextEncoder().encode(digestText(rows)));
}
