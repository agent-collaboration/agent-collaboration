import { parse, stringify } from "lossless-json";
import { z } from "zod";

export function decode(text: string): unknown {
  return parse(text, undefined, (token: string): number | bigint =>
    /^[+-]?\d+$/.test(token) ? BigInt(token) : Number(token),
  );
}
export function encode(value: unknown): string {
  const text = stringify(value);
  if (text === undefined) throw new Error("JSON value required");
  return text;
}
export const objectSchema = z.record(z.string(), z.unknown());
export function field(value: unknown, ...path: readonly string[]): unknown {
  let current = value;
  for (const name of path) current = objectSchema.parse(current)[name];
  return current;
}
export function textField(value: unknown, ...path: readonly string[]): string {
  return z.string().parse(field(value, ...path));
}
export function arrayField(value: unknown, ...path: readonly string[]): unknown[] {
  return z.array(z.unknown()).parse(field(value, ...path));
}
