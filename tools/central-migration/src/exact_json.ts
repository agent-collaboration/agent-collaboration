// JSON that keeps every integer exact. SQLite integers and the library's u64
// fields reach 2^63, beyond a JavaScript number's 2^53; JSON.parse would round
// them silently (9007199254740993 becomes 9007199254740992). Here an integer
// outside the safe range parses as a bigint from its source digits and
// serializes back as those same digits, so a value read from storage, carried
// between this Worker and its objects, compared and written back never changes.

/** A safe integer stays a number; any other integer is a bigint. */
export type ExactInteger = number | bigint;

declare global {
  interface JSON {
    rawJSON(text: string): unknown;
  }
}

const INTEGER_SOURCE = /^-?\d+$/;

export function parseExact(text: string): unknown {
  return JSON.parse(text, function (
    this: unknown,
    _key,
    value: unknown,
    context?: { source?: string },
  ) {
    if (typeof value !== "number" || Number.isSafeInteger(value)) return value;
    const source = context?.source;
    return source !== undefined && INTEGER_SOURCE.test(source) ? BigInt(source) : value;
  } as (this: unknown, key: string, value: unknown) => unknown);
}

/** A JSON number written with a fraction or exponent: serde_json reads it as f64, never as an integer. */
export class JsonFloat {
  readonly source: string;
  readonly value: number;
  constructor(source: string, value: number) {
    this.source = source;
    this.value = value;
  }
}

/**
 * JSON as serde_json reads a stored record: an integer token stays an exact
 * integer, and any other number token (1.0, 1e0) becomes a JsonFloat, so a
 * decoder expecting an integer refuses it as serde's integer visitors do.
 */
export function parseSerdeJson(text: string): unknown {
  return JSON.parse(text, function (
    this: unknown,
    _key,
    value: unknown,
    context?: { source?: string },
  ) {
    if (typeof value !== "number") return value;
    const source = context?.source ?? String(value);
    if (!INTEGER_SOURCE.test(source)) return new JsonFloat(source, value);
    return Number.isSafeInteger(value) ? value : BigInt(source);
  } as (this: unknown, key: string, value: unknown) => unknown);
}

export function stringifyExact(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) =>
    typeof item === "bigint" ? JSON.rawJSON(item.toString()) : item,
  );
}

/** SQLite's own decimal text for an integer, as the smallest exact JavaScript value. */
export function exactInteger(text: string): ExactInteger {
  const value = BigInt(text);
  return value >= BigInt(Number.MIN_SAFE_INTEGER) && value <= BigInt(Number.MAX_SAFE_INTEGER)
    ? Number(value)
    : value;
}

export const I64_MAX = 9_223_372_036_854_775_807n;
export const U64_MAX = 18_446_744_073_709_551_615n;
const CANONICAL_UNSIGNED = /^(0|[1-9]\d*)$/;

/**
 * An unsigned integer exactly, from a stored number, bigint or canonical decimal
 * text, or null when it is anything else (noncanonical text, negative, fractional).
 */
export function exactUnsigned(value: unknown): bigint | null {
  if (typeof value === "bigint") return value >= 0n ? value : null;
  if (typeof value === "number")
    return Number.isSafeInteger(value) && value >= 0 ? BigInt(value) : null;
  if (typeof value === "string" && CANONICAL_UNSIGNED.test(value)) return BigInt(value);
  return null;
}
