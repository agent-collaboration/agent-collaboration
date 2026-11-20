import { z } from "zod";

const typeName = z.string().regex(/^[A-Z][A-Za-z0-9]*$/);
const fieldName = z.string().regex(/^_?[a-z][A-Za-z0-9]*(?:_[a-z0-9]+)*$/);
const enumValue = z.string().regex(/^[a-z][A-Za-z0-9]*(?:[-_][a-z0-9]+)*$/);
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
  additionalProperties: z.literal(false).optional(),
  anyOf: z
    .array(
      z.strictObject({
        type: z.literal("object"),
        title: typeName,
        properties: z.record(fieldName, z.unknown()),
        required: z.tuple([fieldName]),
      }),
    )
    .min(2)
    .optional(),
});
const referenceSchema = z.strictObject({ $ref: definitionRef });
const composedReference = z.strictObject({ allOf: z.tuple([referenceSchema]) });
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
  format: z.enum(["uint16", "uint32", "uint64", "uint"]),
  minimum: z.literal(0),
});
const signedIntegerSchema = z.strictObject({
  type: z.literal("integer"),
  format: z.enum(["int32", "int64"]),
});
const arraySchema = z.strictObject({ type: z.literal("array"), items: z.unknown().nonoptional() });
const mapSchema = z.strictObject({
  type: z.literal("object"),
  additionalProperties: z.unknown().nonoptional(),
});
const openObjectSchema = z.strictObject({ type: z.literal("object") });
const nullableTypes = z.looseObject({
  type: z.tuple([z.enum(["string", "boolean", "integer", "array", "object"]), z.literal("null")]),
});
const nullableReference = z.strictObject({
  anyOf: z.tuple([referenceSchema, z.strictObject({ type: z.literal("null") })]),
});
const taggedObjectSchema = z.strictObject({
  oneOf: z.array(z.unknown()).min(1),
  type: z.literal("object").optional(),
  properties: z.record(fieldName, z.unknown()).optional(),
});
const branchTagSchema = stringSchema.extend({
  enum: z.tuple([enumValue]),
  title: typeName.optional(),
});
const oneOfSchema = z.strictObject({ oneOf: z.array(z.unknown()).min(1) });
const anyOfSchema = z.strictObject({ anyOf: z.array(z.unknown()).min(1) });
const openUnionBranch = z.strictObject({
  type: z.tuple([z.literal("string"), z.literal("object")]),
});

const integerTypes = {
  uint16: "u16",
  uint32: "u32",
  uint64: "u64",
  uint: "usize",
  int32: "i32",
  int64: "i64",
} as const;

export type ValueShape =
  | {
      readonly kind: "scalar";
      readonly rustType: "String" | "bool" | (typeof integerTypes)[keyof typeof integerTypes];
    }
  | { readonly kind: "reference"; readonly pointer: string }
  | { readonly kind: "sequence"; readonly item: ValueShape }
  | { readonly kind: "map"; readonly value: ValueShape }
  | { readonly kind: "branchString" }
  | { readonly kind: "opaque" };

interface ObjectField {
  readonly name: string;
  readonly shape: ValueShape;
  readonly description: string | undefined;
  readonly nullable: boolean;
  readonly required: boolean;
  readonly defaultValue: unknown;
}

export interface ObjectDefinition {
  readonly kind: "object";
  readonly fields: readonly ObjectField[];
  readonly closed: boolean;
  readonly anyRequired: readonly string[];
}

export interface TaggedBranch {
  readonly tag: string;
  readonly name: string;
  readonly description: string | undefined;
  readonly object: ObjectDefinition;
}

export interface WrappedBranch {
  readonly key: string;
  readonly name: string;
  readonly description: string | undefined;
  readonly value: ValueShape | ObjectDefinition;
}

export type Definition =
  | ObjectDefinition
  | { readonly kind: "enum"; readonly variants: readonly string[] }
  | { readonly kind: "nonEmptyString" }
  | { readonly kind: "alias"; readonly shape: ValueShape }
  | { readonly kind: "tagged"; readonly branches: readonly TaggedBranch[] }
  | {
      readonly kind: "wrapped";
      readonly tags: readonly string[];
      readonly branches: readonly WrappedBranch[];
      readonly open: boolean;
    }
  | { readonly kind: "textOrSequence"; readonly item: ValueShape };

export interface NamedDefinition {
  readonly name: string;
  readonly description: string | undefined;
  readonly definition: Definition;
}

export function definitionName(pointer: string): string {
  return definitionRef.parse(pointer).slice("#/definitions/v2/".length);
}

function parseValue(schema: unknown, path: string): ValueShape {
  if (schema === true) return { kind: "opaque" };
  const reference = referenceSchema.safeParse(schema);
  if (reference.success) return { kind: "reference", pointer: reference.data.$ref };
  const composed = composedReference.safeParse(schema);
  if (composed.success) return { kind: "reference", pointer: composed.data.allOf[0].$ref };
  const array = arraySchema.safeParse(schema);
  if (array.success) {
    return { kind: "sequence", item: parseValue(array.data.items, `${path}/items`) };
  }
  const map = mapSchema.safeParse(schema);
  if (map.success) {
    return {
      kind: "map",
      value: parseValue(map.data.additionalProperties, `${path}/additionalProperties`),
    };
  }
  if (stringSchema.safeParse(schema).success) return { kind: "scalar", rustType: "String" };
  if (boolSchema.safeParse(schema).success) return { kind: "scalar", rustType: "bool" };
  const unsigned = unsignedIntegerSchema.safeParse(schema);
  if (unsigned.success) return { kind: "scalar", rustType: integerTypes[unsigned.data.format] };
  const signed = signedIntegerSchema.safeParse(schema);
  if (signed.success) return { kind: "scalar", rustType: integerTypes[signed.data.format] };
  throw new Error(`${path}: unsupported wire value ${JSON.stringify(schema)}`);
}

function parseField(props: {
  path: string;
  name: string;
  schema: unknown;
  required: boolean;
}): ObjectField {
  const { name, path, required } = props;
  if (props.schema === true) {
    return {
      name,
      shape: { kind: "opaque" },
      description: undefined,
      required,
      nullable: false,
      defaultValue: undefined,
    };
  }
  const metadata = schemaMetadata.safeParse(props.schema);
  if (!metadata.success)
    throw new Error(`${path}: unsupported field schema ${JSON.stringify(props.schema)}`);
  const { description, default: defaultValue, ...schema } = metadata.data;
  const types = nullableTypes.safeParse(schema);
  const reference = nullableReference.safeParse(schema);
  const nullable = types.success || reference.success;
  const value = types.success
    ? { ...schema, type: types.data.type[0] }
    : reference.success
      ? reference.data.anyOf[0]
      : schema;
  if (required && defaultValue !== undefined) {
    throw new Error(`${path}: unsupported default on a required field`);
  }
  return { name, shape: parseValue(value, path), description, required, nullable, defaultValue };
}

function parseObject(
  object: z.infer<typeof objectSchema>,
  path: string,
  common: Readonly<Record<string, unknown>> = {},
): ObjectDefinition {
  const properties = { ...common, ...object.properties };
  const required = new Set(object.required ?? []);
  if (
    Object.keys(common).some((name) => Object.hasOwn(object.properties, name)) ||
    required.size !== (object.required?.length ?? 0) ||
    [...required].some((field) => !Object.hasOwn(properties, field))
  ) {
    throw new Error(`${path}: overlapping properties or invalid required fields`);
  }
  const anyRequired: string[] = [];
  if (object.anyOf !== undefined && object.additionalProperties === false) {
    throw new Error(`${path}/anyOf: closed object alternatives are unsupported`);
  }
  for (const alternative of object.anyOf ?? []) {
    const [field] = alternative.required;
    if (
      !Object.hasOwn(alternative.properties, field) ||
      Object.keys(alternative.properties).length !== 1 ||
      Object.hasOwn(properties, field) ||
      !stringSchema.safeParse(alternative.properties[field]).success
    ) {
      throw new Error(`${path}/anyOf: expected separate required string alternatives`);
    }
    properties[field] = alternative.properties[field];
    anyRequired.push(field);
  }
  const fields = Object.keys(properties)
    .sort()
    .map((field) =>
      anyRequired.includes(field)
        ? {
            name: field,
            shape: { kind: "branchString" } as const,
            description: undefined,
            required: false,
            nullable: false,
            defaultValue: undefined,
          }
        : parseField({
            name: field,
            path: `${path}/properties/${field}`,
            schema: properties[field],
            required: required.has(field),
          }),
    );
  return { kind: "object", fields, closed: object.additionalProperties === false, anyRequired };
}

function parseTagged(schema: unknown, path: string): Definition | undefined {
  const union = taggedObjectSchema.safeParse(schema);
  if (!union.success) return undefined;
  const branches: TaggedBranch[] = [];
  for (const [index, branch] of union.data.oneOf.entries()) {
    const branchPath = `${path}/oneOf/${index}`;
    const { title, description, ...shape } = schemaMetadata.parse(branch);
    const object = objectSchema.safeParse(shape);
    if (!object.success || title === undefined) return undefined;
    const { type, ...properties } = object.data.properties;
    const tag = branchTagSchema.safeParse(type);
    if (!tag.success || !object.data.required?.includes("type")) return undefined;
    if (object.data.additionalProperties === false) {
      throw new Error(`${branchPath}: closed tagged branch is unsupported`);
    }
    branches.push({
      tag: tag.data.enum[0],
      name: title,
      description,
      object: parseObject(
        {
          ...object.data,
          properties,
          required: object.data.required.filter((key) => key !== "type"),
        },
        branchPath,
        union.data.properties,
      ),
    });
  }
  if (new Set(branches.map((branch) => branch.tag)).size !== branches.length) {
    throw new Error(`${path}: duplicate tagged union discriminator`);
  }
  return { kind: "tagged", branches };
}

function parseWrapped(shape: unknown, path: string): Definition | undefined {
  const exclusive = oneOfSchema.safeParse(shape);
  const inclusive = anyOfSchema.safeParse(shape);
  const alternatives = exclusive.success
    ? exclusive.data.oneOf
    : inclusive.success
      ? inclusive.data.anyOf
      : [];
  const [first, second] = alternatives;
  const sequence = arraySchema.safeParse(second);
  if (
    inclusive.success &&
    alternatives.length === 2 &&
    stringSchema.safeParse(first).success &&
    sequence.success
  ) {
    return {
      kind: "textOrSequence",
      item: parseValue(sequence.data.items, `${path}/anyOf/1/items`),
    };
  }
  const stringTags = enumSchema.safeParse(first);
  if (!stringTags.success) return undefined;
  const tags = stringTags.data.enum;
  const open = inclusive.success && openUnionBranch.safeParse(alternatives.at(-1)).success;
  if ((inclusive.success && !open) || new Set(tags).size !== tags.length) return undefined;
  const branches: WrappedBranch[] = [];
  for (const [index, branch] of alternatives.slice(1, open ? -1 : undefined).entries()) {
    const branchPath = `${path}/${open ? "anyOf" : "oneOf"}/${index + 1}`;
    const { title, description, ...shape } = schemaMetadata.parse(branch);
    const wrapper = objectSchema.safeParse(shape);
    if (!wrapper.success || title === undefined) return undefined;
    const [key] = wrapper.data.required ?? [];
    if (
      key === undefined ||
      wrapper.data.required?.length !== 1 ||
      Object.keys(wrapper.data.properties).length !== 1 ||
      !Object.hasOwn(wrapper.data.properties, key) ||
      wrapper.data.additionalProperties !== false ||
      wrapper.data.anyOf !== undefined
    ) {
      throw new Error(`${branchPath}: expected a closed single-key object alternative`);
    }
    const inner = wrapper.data.properties[key];
    const object = objectSchema.safeParse(inner);
    const value = object.success
      ? parseObject(object.data, `${branchPath}/properties/${key}`)
      : parseValue(inner, `${branchPath}/properties/${key}`);
    branches.push({ key, name: title, description, value });
  }
  if (new Set(branches.map((branch) => branch.key)).size !== branches.length) {
    throw new Error(`${path}: duplicate wrapped union key`);
  }
  return { kind: "wrapped", tags, branches, open };
}

function parseDefinition(schema: unknown, pointer: string): NamedDefinition {
  const name = definitionName(pointer);
  const { $schema: _dialect, title, description, ...shape } = schemaMetadata.parse(schema);
  if (title !== undefined && title !== name)
    throw new Error(`${pointer}: unexpected title ${title}`);

  const object = objectSchema.safeParse(shape);
  if (object.success) {
    return { name, description, definition: parseObject(object.data, pointer) };
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
  const unionDefinition = parseTagged(shape, pointer) ?? parseWrapped(shape, pointer);
  if (unionDefinition) return { name, description, definition: unionDefinition };
  if (stringSchema.safeParse(shape).success) {
    return {
      name,
      description,
      definition: { kind: "alias", shape: { kind: "scalar", rustType: "String" } },
    };
  }
  if (openObjectSchema.safeParse(shape).success) {
    return {
      name,
      description,
      definition: { kind: "alias", shape: { kind: "map", value: { kind: "opaque" } } },
    };
  }
  throw new Error(`${pointer}: unsupported definition ${JSON.stringify(shape)}`);
}

function references(shape: ValueShape): readonly string[] {
  switch (shape.kind) {
    case "scalar":
    case "opaque":
    case "branchString":
      return [];
    case "reference":
      return [shape.pointer];
    case "sequence":
      return references(shape.item);
    case "map":
      return references(shape.value);
  }
}

function definitionReferences(definition: Definition): readonly string[] {
  switch (definition.kind) {
    case "object":
      return definition.fields.flatMap((field) => references(field.shape));
    case "alias":
      return references(definition.shape);
    case "tagged":
      return definition.branches.flatMap((branch) => definitionReferences(branch.object));
    case "wrapped":
      return definition.branches.flatMap(({ value }) =>
        value.kind === "object" ? definitionReferences(value) : references(value),
      );
    case "textOrSequence":
      return references(definition.item);
    case "enum":
    case "nonEmptyString":
      return [];
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
    for (const child of definitionReferences(parsed.definition)) visit(child);
    visiting.delete(pointer);
    resolved.set(pointer, parsed);
  }

  for (const root of roots) visit(root);
  return resolved;
}
