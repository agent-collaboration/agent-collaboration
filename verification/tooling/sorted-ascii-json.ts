function sortedJsonFields(value: unknown): unknown {
  if (Array.isArray(value)) return value.map((entry: unknown) => sortedJsonFields(entry));
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
        .map(([key, entry]) => [key, sortedJsonFields(entry)]),
    );
  return value;
}

export function sortedAsciiJson(value: unknown): string {
  const serialized = JSON.stringify(sortedJsonFields(value), null, 2);
  if (serialized === undefined) throw new Error("value cannot be serialized as JSON");
  return `${serialized.replace(/[\u007f-\uffff]/g, (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`)}\n`;
}
