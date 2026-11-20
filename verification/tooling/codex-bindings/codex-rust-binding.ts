import { definitionName, type NamedDefinition, type ValueShape } from "./codex-schema.ts";

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
  return `${value.charAt(0).toUpperCase()}${value.slice(1)}`;
}

function rustType(shape: ValueShape): string {
  switch (shape.kind) {
    case "scalar":
      return shape.rustType;
    case "reference":
      return definitionName(shape.pointer);
    case "sequence":
      return `Vec<${rustType(shape.item)}>`;
  }
}

function rustDocs(description: string | undefined): string[] {
  return description?.split(/\r?\n/).map((line) => `/// ${line}`.trimEnd()) ?? [];
}

function renderField(field: StructField): string[] {
  const type = rustType(field.shape);
  const rename = `rename = "${field.name}"`;
  const attributes = field.required
    ? [`#[serde(${rename})]`]
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

function renderDefinition(context: DefaultContext): string {
  const { name, description, definition } = context.definition;
  const docs = rustDocs(description);
  const derives = "#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]";
  switch (definition.kind) {
    case "enum":
      return [
        ...docs,
        derives,
        `pub enum ${name} {`,
        ...definition.variants.flatMap((value) => [
          `#[serde(rename = "${value}")]`,
          `${variantIdentifier(value)},`,
        ]),
        "}",
      ].join("\n");
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
    case "object": {
      const identifiers = definition.fields.map((field) => fieldIdentifier(field.name));
      if (new Set(identifiers).size !== identifiers.length) {
        throw new Error(`${name}: duplicate Rust field identifiers`);
      }
      const defaults = definition.fields.flatMap((field) => renderDefault(field, context));
      return [
        "codex_object! {",
        ...docs.map((line) => `    ${line}`),
        `    ${name} {`,
        ...definition.fields.flatMap(renderField).map((line) => `        ${line}`),
        "    }",
        "}",
        ...(defaults.length === 0 ? [] : ["", `impl ${name} {`, ...defaults, "}"]),
      ].join("\n");
    }
  }
}

interface RequestBinding {
  readonly method: string;
  readonly params: string;
  readonly response: string;
}

export function renderRustBindings(
  binding: RequestBinding,
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
      renderDefinition({ definition, definitions }),
      "",
    ]),
    `impl super::sealed::Sealed for ${definitionName(binding.params)} {}`,
    "",
    `impl super::CodexRequest for ${definitionName(binding.params)} {`,
    `const METHOD: &'static str = "${binding.method}";`,
    `type Response = ${definitionName(binding.response)};`,
    "}",
    "",
  ].join("\n");
}
