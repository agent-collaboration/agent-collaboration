import {
  definitionName,
  type Definition,
  type NamedDefinition,
  type ObjectDefinition,
  type ValueShape,
} from "./codex-schema.ts";

type StructField = Extract<NamedDefinition["definition"], { kind: "object" }>["fields"][number];

interface DefaultContext {
  readonly definition: NamedDefinition;
  readonly definitions: ReadonlyMap<string, NamedDefinition>;
}

function fieldIdentifier(name: string): string {
  return name
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

function variantIdentifier(value: string): string {
  return value.replace(/(?:^|[-_])([a-z0-9])/g, (_, letter: string) => letter.toUpperCase());
}

function taggedVariantIdentifiers(tags: readonly string[]): readonly string[] {
  const identifiers = tags.map(variantIdentifier);
  const prefix = identifiers[0]?.match(/^[A-Z][a-z0-9]+(?=[A-Z])/)?.[0];
  if (
    identifiers.length < 3 ||
    prefix === undefined ||
    !identifiers.every(
      (name) => name.startsWith(prefix) && /^[A-Z]/.test(name.slice(prefix.length)),
    )
  ) {
    return identifiers;
  }
  return identifiers.map((name) => name.slice(prefix.length));
}

function rustType(shape: ValueShape): string {
  switch (shape.kind) {
    case "scalar":
      return shape.rustType;
    case "reference":
      return definitionName(shape.pointer);
    case "sequence":
      return `Vec<${rustType(shape.item)}>`;
    case "map":
      return `std::collections::BTreeMap<String, ${rustType(shape.value)}>`;
    case "branchString":
      return "super::StringAlternative";
    case "opaque":
      return "serde_json::Value";
  }
}

function rustDocs(description: string | undefined): string[] {
  return description?.split(/\r?\n/).map((line) => `/// ${line}`.trimEnd()) ?? [];
}

function renderField(field: StructField): string[] {
  const type = rustType(field.shape);
  const rename = `rename = "${field.name}"`;
  const attributes =
    field.required && !field.nullable
      ? [`#[serde(${rename})]`]
      : field.required
        ? [`#[serde(${rename}, deserialize_with = "super::deserialize_required_nullable")]`]
        : [
            "#[serde(",
            `    ${rename},`,
            "    default,",
            '    skip_serializing_if = "Option::is_none",',
            '    deserialize_with = "super::deserialize_present",',
            ")]",
          ];
  const value = field.nullable ? `Option<${type}>` : type;
  return [
    ...rustDocs(field.description),
    ...attributes,
    `${fieldIdentifier(field.name)}: ${field.required ? value : `Option<${value}>`},`,
  ];
}

function defaultSequence(props: {
  context: DefaultContext;
  field: StructField;
  item: ValueShape;
  values: readonly unknown[];
}): string {
  const { context, field, item, values } = props;
  if (values.length === 0) return "&[]";
  if (item.kind === "reference") {
    const target = context.definitions.get(item.pointer);
    if (target?.definition.kind === "enum") {
      const variants = new Set(target.definition.variants);
      const defaults = values.map((value) => {
        if (typeof value !== "string" || !variants.has(value)) {
          throw unsupportedDefault(context, field);
        }
        return `${target.name}::${variantIdentifier(value)}`;
      });
      return `&[${defaults.join(", ")}]`;
    }
  }
  throw unsupportedDefault(context, field);
}

function unsupportedDefault(context: DefaultContext, field: StructField): Error {
  return new Error(`${context.definition.name}.${field.name}: unsupported default value`);
}

function renderDefault(field: StructField, context: DefaultContext): string[] {
  const { defaultValue, shape } = field;
  if (defaultValue === undefined) return [];
  const name = fieldIdentifier(field.name);
  let result: string;
  let expression: string;
  if (field.nullable && defaultValue === null) {
    const type = rustType(shape);
    result = `Option<&${type === "String" ? "str" : type}>`;
    const accessor = type === "String" ? "as_deref" : "as_ref";
    expression = `self.${name}.as_ref().and_then(Option::${accessor})`;
  } else if (
    !field.nullable &&
    shape.kind === "scalar" &&
    shape.rustType === "bool" &&
    typeof defaultValue === "boolean"
  ) {
    result = "bool";
    expression = `self.${name}.unwrap_or(${defaultValue})`;
  } else if (!field.nullable && shape.kind === "sequence" && Array.isArray(defaultValue)) {
    result = `&[${rustType(shape.item)}]`;
    const fallback = defaultSequence({ context, field, item: shape.item, values: defaultValue });
    expression = `self.${name}.as_deref().unwrap_or(${fallback})`;
  } else if (!field.nullable && shape.kind === "reference" && typeof defaultValue === "string") {
    const reference = context.definitions.get(shape.pointer);
    const enumeration = reference?.definition;
    result = rustType(shape);
    const variant = variantIdentifier(defaultValue);
    if (enumeration?.kind === "enum" && enumeration.variants.includes(defaultValue)) {
      expression = `self.${name}.clone().unwrap_or(${result}::${variant})`;
    } else if (enumeration?.kind === "wrapped" && enumeration.tags.includes(defaultValue)) {
      expression = `self.${name}.clone().unwrap_or(${result}::Known(${result}Tag::${variant}))`;
    } else {
      throw unsupportedDefault(context, field);
    }
  } else {
    throw unsupportedDefault(context, field);
  }
  return [
    "/// Effective value using the schema default when the field is absent.",
    "#[must_use]",
    `pub fn ${name}(&self) -> ${result} {`,
    expression,
    "}",
    "",
  ];
}

function renderStringEnum(
  name: string,
  variants: readonly string[],
  docs: readonly string[],
): string {
  const expectedTags = variants.map((value) => `"${value}"`).join(", ");
  const identifiers = variants.map(variantIdentifier);
  if (new Set(identifiers).size !== identifiers.length) {
    throw new Error(`${name}: duplicate Rust enum variant names`);
  }
  return [
    ...docs,
    "#[derive(Clone, Debug, Eq, PartialEq, Serialize)]",
    `pub enum ${name} {`,
    ...variants.flatMap((value) => [
      `#[serde(rename = "${value}")]`,
      `${variantIdentifier(value)},`,
    ]),
    "}",
    "",
    `impl<'de> Deserialize<'de> for ${name} {`,
    "fn deserialize<Decoder: serde::Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {",
    "let tag = String::deserialize(decoder)?;",
    "match tag.as_str() {",
    ...variants.map((value) => `"${value}" => Ok(Self::${variantIdentifier(value)}),`),
    `_ => Err(serde::de::Error::unknown_variant(&tag, &[${expectedTags}])),`,
    "}",
    "}",
    "}",
  ].join("\n");
}

function renderObject(context: DefaultContext, object: ObjectDefinition): string {
  const { name, description } = context.definition;
  const identifiers = object.fields.map((field) => fieldIdentifier(field.name));
  if (new Set(identifiers).size !== identifiers.length) {
    throw new Error(`${name}: duplicate Rust field identifiers`);
  }
  const defaults = object.fields.flatMap((field) => renderDefault(field, context));
  const constraint = object.closed
    ? ["    closed"]
    : object.anyRequired.length === 0
      ? []
      : [`    choice [${object.anyRequired.map(fieldIdentifier).join(", ")}]`];
  return [
    "codex_object! {",
    ...constraint,
    ...rustDocs(description).map((line) => `    ${line}`),
    `    ${name} {`,
    ...object.fields.flatMap(renderField).map((line) => `        ${line}`),
    "    }",
    "}",
    ...(defaults.length === 0 ? [] : ["", `impl ${name} {`, ...defaults, "}"]),
  ].join("\n");
}

function renderTagged(
  context: DefaultContext,
  tagged: Extract<Definition, { kind: "tagged" }>,
): string {
  const { name, description } = context.definition;
  const identifiers = taggedVariantIdentifiers(tagged.branches.map((branch) => branch.tag));
  if (new Set(identifiers).size !== identifiers.length) {
    throw new Error(`${name}: duplicate Rust tagged variant names`);
  }
  return [
    ...tagged.branches.flatMap((branch) => [
      renderObject(
        {
          ...context,
          definition: {
            name: branch.name,
            description: branch.description,
            definition: branch.object,
          },
        },
        branch.object,
      ),
      "",
    ]),
    "codex_tagged_object! {",
    ...rustDocs(description).map((line) => `    ${line}`),
    `    ${name} {`,
    ...tagged.branches.map(({ tag, name: payload }, index) => {
      const variant = identifiers[index];
      if (variant === undefined) throw new Error(`${name}: missing Rust tagged variant name`);
      return `        "${tag}" => ${variant}(${payload}),`;
    }),
    "    }",
    "}",
  ].join("\n");
}

function renderWrapped(
  context: DefaultContext,
  wrapped: Extract<Definition, { kind: "wrapped" }>,
): string {
  const { name, description } = context.definition;
  const tagName = `${name}Tag`;
  const wrappers = wrapped.branches.flatMap((branch) => {
    const valueName = `${branch.name}Payload`;
    const value = branch.value;
    const field = {
      name: branch.key,
      required: true,
      nullable: false,
      defaultValue: undefined,
      description: undefined,
      shape: value.kind === "object" ? undefined : value,
    };
    const prefix =
      value.kind === "object"
        ? [
            renderObject(
              {
                ...context,
                definition: { name: valueName, description: undefined, definition: value },
              },
              value,
            ),
            "",
          ]
        : [];
    return [
      ...prefix,
      "codex_object! {",
      "    closed",
      ...rustDocs(branch.description).map((line) => `    ${line}`),
      `    ${branch.name} {`,
      ...(field.shape === undefined
        ? [
            `        #[serde(rename = "${branch.key}")]`,
            `        ${fieldIdentifier(branch.key)}: ${valueName},`,
          ]
        : renderField({ ...field, shape: field.shape }).map((line) => `        ${line}`)),
      "    }",
      "}",
      "",
    ];
  });
  const variants = wrapped.branches.map(
    (branch) => `${variantIdentifier(branch.key)}(${branch.name}),`,
  );
  const openVariants = wrapped.open
    ? [
        "UnknownString(String),",
        "UnknownObject(serde_json::Map<String, serde_json::Value>),",
        "MalformedKnownObject(serde_json::Map<String, serde_json::Value>),",
      ]
    : [];
  return [
    renderStringEnum(tagName, wrapped.tags, []),
    "",
    ...wrappers,
    ...rustDocs(description),
    wrapped.open
      ? "#[derive(Clone, Debug, Eq, PartialEq, Serialize)]"
      : "#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]",
    "#[serde(untagged)]",
    `pub enum ${name} {`,
    `Known(${tagName}),`,
    ...variants,
    ...openVariants,
    "}",
    ...(wrapped.open ? renderOpenWrapped(name, tagName, wrapped) : []),
  ].join("\n");
}

function renderOpenWrapped(
  name: string,
  tagName: string,
  wrapped: Extract<Definition, { kind: "wrapped" }>,
): string[] {
  return [
    "",
    `impl<'de> Deserialize<'de> for ${name} {`,
    "fn deserialize<Decoder: serde::Deserializer<'de>>(decoder: Decoder) -> Result<Self, Decoder::Error> {",
    "match serde_json::Value::deserialize(decoder)? {",
    "serde_json::Value::String(tag) => Ok(match tag.as_str() {",
    ...wrapped.tags.map((tag) => `"${tag}" => Self::Known(${tagName}::${variantIdentifier(tag)}),`),
    "_ => Self::UnknownString(tag),",
    "}),",
    "serde_json::Value::Object(fields) => {",
    ...wrapped.branches.flatMap((branch) => [
      `if fields.contains_key("${branch.key}") {`,
      "let value = serde_json::Value::Object(fields.clone());",
      `return Ok(match serde_json::from_value::<${branch.name}>(value) {`,
      `Ok(payload) => Self::${variantIdentifier(branch.key)}(payload),`,
      "Err(_) => Self::MalformedKnownObject(fields),",
      "});",
      "}",
    ]),
    "Ok(Self::UnknownObject(fields))",
    "},",
    '_ => Err(serde::de::Error::custom("expected a string or object")),',
    "}",
    "}",
    "}",
  ];
}

function renderDefinition(context: DefaultContext): string {
  const { name, description, definition } = context.definition;
  const docs = rustDocs(description);
  const derives = "#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]";
  switch (definition.kind) {
    // Derived enum deserialization also accepts object-shaped tags. Decode the schema string first.
    case "enum":
      return renderStringEnum(name, definition.variants, docs);
    case "nonEmptyString":
      return [
        ...docs,
        derives,
        '#[serde(try_from = "String")]',
        `pub struct ${name}(String);`,
        "",
        `impl ${name} {`,
        "#[must_use]",
        "pub fn as_str(&self) -> &str { &self.0 }",
        "}",
        "",
        `impl TryFrom<String> for ${name} {`,
        "type Error = &'static str;",
        "",
        "fn try_from(value: String) -> Result<Self, Self::Error> {",
        'if value.is_empty() { return Err("expected a non-empty string"); }',
        "Ok(Self(value))",
        "}",
        "}",
      ].join("\n");
    case "object":
      return renderObject(context, definition);
    case "alias":
      return [...docs, `pub type ${name} = ${rustType(definition.shape)};`].join("\n");
    case "tagged":
      return renderTagged(context, definition);
    case "wrapped":
      return renderWrapped(context, definition);
    case "textOrSequence":
      return [
        ...docs,
        derives,
        "#[serde(untagged)]",
        `pub enum ${name} {`,
        "Text(String),",
        `Items(Vec<${rustType(definition.item)}>),`,
        "}",
      ].join("\n");
  }
}

interface RequestBinding {
  readonly method: string;
  readonly params: string;
  readonly response: string;
}

export function renderRustBindings(
  bindings: readonly RequestBinding[],
  definitions: ReadonlyMap<string, NamedDefinition>,
): string {
  const orderedDefinitions = [...definitions.values()].sort((left, right) =>
    left.name < right.name ? -1 : left.name > right.name ? 1 : 0,
  );
  return [
    "// @generated by verification/tooling/codex-bindings/generate-codex-bindings.ts.",
    "// Source identities and hashes are recorded in verification/schemas/pins.json.",
    "",
    "use serde::{Deserialize, Serialize};",
    "",
    ...orderedDefinitions.flatMap((definition) => [
      `// #/definitions/v2/${definition.name}`,
      renderDefinition({ definition, definitions }),
      "",
    ]),
    ...bindings.flatMap((binding) => [
      `impl super::sealed::Sealed for ${definitionName(binding.params)} {}`,
      "",
      `impl super::CodexRequest for ${definitionName(binding.params)} {`,
      `const METHOD: &'static str = "${binding.method}";`,
      `type Response = ${definitionName(binding.response)};`,
      "}",
      "",
    ]),
  ].join("\n");
}
