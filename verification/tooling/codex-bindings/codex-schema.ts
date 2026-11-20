import { z } from "zod";

const typeName = z.string().regex(/^[A-Z][A-Za-z0-9]*$/);
const fieldName = z.string().regex(/^[a-z][A-Za-z0-9]*$/);
const enumValue = z.string().regex(/^[a-z][A-Za-z0-9]*$/);
export const definitionRef = z.string().regex(/^#\/definitions\/v2\/[A-Z][A-Za-z0-9]*$/);

const schemaMetadata = z.looseObject({
  $schema: z.literal("http://json-schema.org/draft-07/schema#").optional(),
  title: typeName.optional(),
  description: z.string().optional(),
  default: z.unknown().optional(),
});
const objectSchema = z.strictObject({
  type: z.literal("object"),
  properties: z.record(fieldName, z.unknown()),
  required: z.array(fieldName).optional(),
});
const referenceSchema = z.strictObject({ $ref: definitionRef });
const stringSchema = z.strictObject({ type: z.literal("string") });
const nonEmptyStringSchema = stringSchema.extend({ minLength: z.literal(1) });
const enumSchema = stringSchema.extend({ enum: z.array(enumValue).min(1) });
const enumBranchSchema = stringSchema.extend({
  enum: z.tuple([enumValue]),
  description: z.string().optional(),
});
const enumUnionSchema = z.strictObject({ oneOf: z.array(enumBranchSchema).min(1) });
const boolSchema = z.strictObject({ type: z.literal("boolean") });
const unsignedIntegerSchema = z.strictObject({
  type: z.literal("integer"),
  format: z.literal("uint32"),
  minimum: z.literal(0),
});
const signedIntegerSchema = z.strictObject({
  type: z.literal("integer"),
  format: z.literal("int64"),
});
const arraySchema = z.strictObject({ type: z.literal("array"), items: z.unknown().nonoptional() });
const nullableTypes = z.looseObject({
  type: z.tuple([z.enum(["string", "boolean", "integer"]), z.literal("null")]),
});
const nullableReference = z.strictObject({
  anyOf: z.tuple([referenceSchema, z.strictObject({ type: z.literal("null") })]),
});

export type ValueShape =
  | { readonly kind: "scalar"; readonly rustType: "String" | "bool" | "u32" | "i64" }
  | { readonly kind: "reference"; readonly pointer: string }
  | { readonly kind: "sequence"; readonly item: ValueShape };

interface ObjectField {
  readonly name: string;
  readonly shape: ValueShape;
  readonly description: string | undefined;
  readonly nullable: boolean;
  readonly required: boolean;
  readonly defaultValue: unknown;
}

export type Definition =
  | { readonly kind: "object"; readonly fields: readonly ObjectField[] }
  | { readonly kind: "enum"; readonly variants: readonly string[] }
  | { readonly kind: "nonEmptyString" };

export interface NamedDefinition {
  readonly name: string;
  readonly description: string | undefined;
  readonly definition: Definition;
}

export function definitionName(pointer: string): string {
  return definitionRef.parse(pointer).slice("#/definitions/v2/".length);
}

function parseValue(schema: unknown, path: string): ValueShape {
  const reference = referenceSchema.safeParse(schema);
  if (reference.success) return { kind: "reference", pointer: reference.data.$ref };
  const array = arraySchema.safeParse(schema);
  if (array.success) {
    return { kind: "sequence", item: parseValue(array.data.items, `${path}/items`) };
  }
  if (stringSchema.safeParse(schema).success) return { kind: "scalar", rustType: "String" };
  if (boolSchema.safeParse(schema).success) return { kind: "scalar", rustType: "bool" };
  if (unsignedIntegerSchema.safeParse(schema).success) return { kind: "scalar", rustType: "u32" };
  if (signedIntegerSchema.safeParse(schema).success) return { kind: "scalar", rustType: "i64" };
  throw new Error(`${path}: unsupported wire value ${JSON.stringify(schema)}`);
}

function parseField(props: {
  path: string;
  name: string;
  schema: unknown;
  required: boolean;
}): ObjectField {
  const { name, path, required } = props;
  const { description, default: defaultValue, ...schema } = schemaMetadata.parse(props.schema);
  const types = nullableTypes.safeParse(schema);
  const reference = nullableReference.safeParse(schema);
  const nullable = types.success || reference.success;
  const value = types.success
    ? { ...schema, type: types.data.type[0] }
    : reference.success
      ? reference.data.anyOf[0]
      : schema;
  if (required && (nullable || defaultValue !== undefined)) {
    throw new Error(`${path}: unsupported nullable or defaulted required field`);
  }
  return { name, shape: parseValue(value, path), description, required, nullable, defaultValue };
}

function parseDefinition(schema: unknown, pointer: string): NamedDefinition {
  const name = definitionName(pointer);
  const { $schema: _dialect, title, description, ...shape } = schemaMetadata.parse(schema);
  if (title !== undefined && title !== name)
    throw new Error(`${pointer}: unexpected title ${title}`);

  const object = objectSchema.safeParse(shape);
  if (object.success) {
    const required = new Set(object.data.required ?? []);
    if (
      required.size !== (object.data.required?.length ?? 0) ||
      [...required].some((field) => !Object.hasOwn(object.data.properties, field))
    ) {
      throw new Error(`${pointer}: required fields must be unique declared properties`);
    }
    const fields = Object.keys(object.data.properties)
      .sort()
      .map((field) =>
        parseField({
          name: field,
          path: `${pointer}/properties/${field}`,
          schema: object.data.properties[field],
          required: required.has(field),
        }),
      );
    return { name, description, definition: { kind: "object", fields } };
  }
  if (nonEmptyStringSchema.safeParse(shape).success) {
    return { name, description, definition: { kind: "nonEmptyString" } };
  }
  const enumeration = enumSchema.safeParse(shape);
  const union = enumUnionSchema.safeParse(shape);
  const variants = enumeration.success
    ? enumeration.data.enum
    : union.success
      ? union.data.oneOf.map((branch) => branch.enum[0])
      : undefined;
  if (variants !== undefined && new Set(variants).size === variants.length) {
    return { name, description, definition: { kind: "enum", variants } };
  }
  throw new Error(`${pointer}: unsupported definition ${JSON.stringify(shape)}`);
}

function references(shape: ValueShape): readonly string[] {
  switch (shape.kind) {
    case "scalar":
      return [];
    case "reference":
      return [shape.pointer];
    case "sequence":
      return references(shape.item);
  }
}

export function reachableDefinitions(
  definitions: Readonly<Record<string, unknown>>,
  roots: readonly string[],
): ReadonlyMap<string, NamedDefinition> {
  const resolved = new Map<string, NamedDefinition>();
  const visiting = new Set<string>();

  function visit(pointer: string): void {
    if (resolved.has(pointer)) return;
    if (visiting.has(pointer)) throw new Error(`${pointer}: unsupported recursive definition`);
    const name = definitionName(pointer);
    if (!Object.hasOwn(definitions, name)) throw new Error(`${pointer}: missing definition`);
    const parsed = parseDefinition(definitions[name], pointer);
    visiting.add(pointer);
    if (parsed.definition.kind === "object") {
      for (const field of parsed.definition.fields) {
        for (const child of references(field.shape)) visit(child);
      }
    }
    visiting.delete(pointer);
    resolved.set(pointer, parsed);
  }

  for (const root of roots) visit(root);
  return resolved;
}
