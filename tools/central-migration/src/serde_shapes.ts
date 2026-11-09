// Decoders that accept exactly what the library's serde derives accept, so the
// migration can refuse a carried value before writing it instead of main's
// first open refusing it after (R7, PD6). Each decoder names the attribute it
// mirrors: `deny_unknown_fields`, `tag`/`content`, `rename_all`, and the
// newtypes' `try_from` checks. Values arrive from exact JSON (integers beyond
// 2^53 as bigints).

import { I64_MAX, U64_MAX } from "./exact_json.ts";

export class Invalid extends Error {}
export const invalid = (path: string, why: string): never => {
  throw new Invalid(`${path}: ${why}`);
};

export type Decoder<T> = ((value: unknown, path: string) => T) & { readonly optional?: true };

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

// Primitive serde types.

function integer(value: unknown, path: string, min: bigint, max: bigint, name: string): bigint {
  const exact =
    typeof value === "bigint"
      ? value
      : typeof value === "number" && Number.isSafeInteger(value)
        ? BigInt(value)
        : invalid(path, `expected ${name}`);
  return exact >= min && exact <= max ? exact : invalid(path, `${name} out of range`);
}
export const u64: Decoder<bigint> = (value, path) => integer(value, path, 0n, U64_MAX, "u64");
export const u32: Decoder<bigint> = (value, path) =>
  integer(value, path, 0n, 4_294_967_295n, "u32");
export const i64: Decoder<bigint> = (value, path) =>
  integer(value, path, -I64_MAX - 1n, I64_MAX, "i64");
/** `Epoch(u64)`, `try_from` refuses 0 (fleet-sync-protocol replication_ids.rs). */
export const epoch: Decoder<bigint> = (value, path) => {
  const exact = u64(value, path);
  return exact > 0n ? exact : invalid(path, "must be at least 1");
};
export const nonZeroU64 = epoch;
/** `SyncTime(i64)`, transparent milliseconds. */
export const syncTime = i64;
export const bool: Decoder<boolean> = (value, path) =>
  typeof value === "boolean" ? value : invalid(path, "expected a boolean");
export const string: Decoder<string> = (value, path) =>
  typeof value === "string" ? value : invalid(path, "expected a string");

const utf8Length = (value: string): number => new TextEncoder().encode(value).length;
/** A `try_from = "String"` newtype bounded in UTF-8 bytes. */
export const text =
  (minBytes: number, maxBytes: number): Decoder<string> =>
  (value, path) => {
    const result = string(value, path);
    const length = utf8Length(result);
    return length >= minBytes && length <= maxBytes
      ? result
      : invalid(path, `expected ${minBytes} to ${maxBytes} bytes`);
  };
export const nonEmptyText = text(1, Number.MAX_SAFE_INTEGER);

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
/** `canonical_uuid_v7`: lowercase hyphenated, version 7, RFC 4122 variant (runtime-primitives call_identity.rs). */
export const uuidV7: Decoder<string> = (value, path) =>
  typeof value === "string" && UUID_V7.test(value)
    ? value
    : invalid(path, "expected a canonical UUIDv7");
/** `PieceDigest`: 64 lowercase hexadecimal characters. */
export const digest: Decoder<string> = (value, path) =>
  typeof value === "string" && /^[0-9a-f]{64}$/.test(value)
    ? value
    : invalid(path, "expected 64 lowercase hexadecimal characters");

/** `Option<T>`: null, or absent as a struct field. */
export function option<T>(inner: Decoder<T>): Decoder<T | null> {
  const decoder = ((value: unknown, path: string) =>
    value === undefined || value === null ? null : inner(value, path)) as Decoder<T | null>;
  return Object.assign(decoder, { optional: true as const });
}

export function vec<T>(inner: Decoder<T>): Decoder<T[]> {
  return (value, path) =>
    Array.isArray(value)
      ? value.map((item, index) => inner(item, `${path}[${index}]`))
      : invalid(path, "expected an array");
}

/** A unit-only enum serialized as its renamed variant name. */
export function unitEnum<T extends string>(names: readonly T[]): Decoder<T> {
  return (value, path) =>
    typeof value === "string" && (names as readonly string[]).includes(value)
      ? (value as T)
      : invalid(path, `expected one of ${names.join(", ")}`);
}

export type Fields = Readonly<Record<string, Decoder<unknown>>>;
export type Decoded<F extends Fields> = {
  [K in keyof F]: F[K] extends Decoder<infer T> ? T : never;
};

function fieldsOf<F extends Fields>(
  value: Record<string, unknown>,
  fields: F,
  path: string,
  denyUnknown: boolean,
  allowed: readonly string[] = [],
): Decoded<F> {
  if (denyUnknown) {
    for (const key of Object.keys(value)) {
      if (!(key in fields) && !allowed.includes(key)) invalid(path, `unknown field ${key}`);
    }
  }
  const out: Record<string, unknown> = {};
  for (const [key, decoder] of Object.entries(fields)) {
    if (!(key in value) && decoder.optional !== true) invalid(path, `missing field ${key}`);
    out[key] = decoder(value[key], `${path}.${key}`);
  }
  return out as Decoded<F>;
}

/** A struct; `denyUnknown` mirrors `#[serde(deny_unknown_fields)]`. Field names are as serialized. */
export function struct<F extends Fields>(fields: F, denyUnknown: boolean): Decoder<Decoded<F>> {
  return (value, path) =>
    isObject(value)
      ? fieldsOf(value, fields, path, denyUnknown)
      : invalid(path, "expected an object");
}

/** A variant's payload: a struct's fields, a newtype's inner value, or nothing. */
export type Variant =
  | { readonly kind: "unit" }
  | { readonly kind: "newtype"; readonly inner: Decoder<unknown> }
  | { readonly kind: "struct"; readonly fields: Fields };
export const unit: Variant = { kind: "unit" };
export const newtype = (inner: Decoder<unknown>): Variant => ({ kind: "newtype", inner });
export const fields = (shape: Fields): Variant => ({ kind: "struct", fields: shape });

export interface Tagged {
  readonly variant: string;
  readonly value: unknown;
}

/**
 * `#[serde(tag = T, content = C, deny_unknown_fields)]`: `{T: name, C: payload}`.
 * A unit variant's content may be absent or null; struct variants deny unknown fields too.
 */
export function adjacentlyTagged(
  tag: string,
  content: string,
  variants: Readonly<Record<string, Variant>>,
): Decoder<Tagged> {
  return (value, path) => {
    if (!isObject(value)) return invalid(path, "expected an object");
    for (const key of Object.keys(value)) {
      if (key !== tag && key !== content) invalid(path, `unknown field ${key}`);
    }
    const name = value[tag];
    const variant = typeof name === "string" ? variants[name] : undefined;
    if (variant === undefined) return invalid(`${path}.${tag}`, `unknown variant ${String(name)}`);
    const payload = value[content];
    const at = `${path}.${content}`;
    switch (variant.kind) {
      case "unit":
        return payload === undefined || payload === null
          ? { variant: String(name), value: null }
          : invalid(at, "unit variant has content");
      case "newtype":
        return payload === undefined
          ? invalid(path, `missing field ${content}`)
          : { variant: String(name), value: variant.inner(payload, at) };
      case "struct":
        return payload === undefined
          ? invalid(path, `missing field ${content}`)
          : isObject(payload)
            ? { variant: String(name), value: fieldsOf(payload, variant.fields, at, true) }
            : invalid(at, "expected an object");
    }
  };
}

/** `#[serde(tag = T, deny_unknown_fields)]` with unit and struct variants: `{T: name, ...fields}`. */
export function internallyTagged(
  tag: string,
  variants: Readonly<Record<string, Fields>>,
): Decoder<Tagged> {
  return (value, path) => {
    if (!isObject(value)) return invalid(path, "expected an object");
    const name = value[tag];
    const shape = typeof name === "string" ? variants[name] : undefined;
    if (shape === undefined) return invalid(`${path}.${tag}`, `unknown variant ${String(name)}`);
    return { variant: String(name), value: fieldsOf(value, shape, path, true, [tag]) };
  };
}

/** A stored JSON text decoded with `decoder`, as the library's `serde_json::from_str`. */
export function fromJsonText<T>(parse: (text: string) => unknown, decoder: Decoder<T>): Decoder<T> {
  return (value, path) => {
    const textValue =
      typeof value === "string" ? value : invalid(path, "missing or not stored as text");
    let parsed: unknown;
    try {
      parsed = parse(textValue);
    } catch {
      return invalid(path, "not JSON");
    }
    return decoder(parsed, path);
  };
}
