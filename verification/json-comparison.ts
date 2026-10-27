import { objectSchema } from "./exact-json.ts";

export function containsJsonSubset(actual: unknown, expected: unknown): boolean {
  if (Array.isArray(expected))
    return (
      Array.isArray(actual) &&
      actual.length === expected.length &&
      expected.every((entry: unknown, index: number) => containsJsonSubset(actual[index], entry))
    );
  if (expected !== null && typeof expected === "object") {
    const parsed = objectSchema.safeParse(actual);
    return (
      parsed.success &&
      Object.entries(expected).every(
        ([key, value]) =>
          Object.hasOwn(parsed.data, key) && containsJsonSubset(parsed.data[key], value),
      )
    );
  }
  if (typeof actual === "boolean" && typeof expected === "number")
    return Number(actual) === expected;
  if (typeof actual === "boolean" && typeof expected === "bigint")
    return BigInt(Number(actual)) === expected;
  if (typeof expected === "boolean" && typeof actual === "number")
    return actual === Number(expected);
  if (typeof expected === "boolean" && typeof actual === "bigint")
    return actual === BigInt(Number(expected));
  if (typeof actual === "number" && typeof expected === "bigint")
    return Number.isInteger(actual) && BigInt(actual) === expected;
  if (typeof actual === "bigint" && typeof expected === "number")
    return Number.isInteger(expected) && actual === BigInt(expected);
  return actual === expected;
}

export function sameJsonContent(left: unknown, right: unknown): boolean {
  return containsJsonSubset(left, right) && containsJsonSubset(right, left);
}
