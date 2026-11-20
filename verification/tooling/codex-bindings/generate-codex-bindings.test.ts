import assert from "node:assert/strict";
import { copyFileSync, mkdtempDisposableSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import { codexBundleSchema } from "../schema-audit/codex-pairing-transcription.ts";
import {
  checkCodexBindings,
  readPinnedCodexInput,
  renderCodexBindings,
} from "./generate-codex-bindings.ts";

const schemas = fileURLToPath(new URL("../../schemas/", import.meta.url));
const committed = fileURLToPath(
  new URL("../../../crates/vendor-protocol/src/codex/generated.rs", import.meta.url),
);
const schema = readPinnedCodexInput(schemas, "codexAppServer");
const pairing = readPinnedCodexInput(schemas, "codexAppServerPairing");

test("two offline generations match the committed shared binding closure", (): void => {
  const generated = renderCodexBindings(schema, pairing);
  assert.equal(renderCodexBindings(schema, pairing), generated);
  const definitions = generated.match(/^\/\/ #\/definitions\/v2\/\w+$/gm);
  assert.equal(definitions?.length, 108);
  assert.equal(new Set(definitions).size, 108);
  assert.equal(generated.match(/impl super::CodexRequest for /g)?.length, 4);
  checkCodexBindings(committed, generated);
});

test("offline drift check rejects an edited artifact and leaves its bytes intact", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-binding-"));
  const output = join(scratch.path, "generated.rs");
  const edited = readFileSync(committed, "utf8").replace(
    'rename = "nextCursor"',
    'rename = "wrongCursor"',
  );
  writeFileSync(output, edited);

  assert.throws(
    () => checkCodexBindings(output, renderCodexBindings(schema, pairing)),
    /generated Codex bindings differ/,
  );
  assert.equal(readFileSync(output, "utf8"), edited);
});

test("changed schema and a pairing pointed at another existing type fail the pins", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-inputs-"));
  copyFileSync(join(schemas, "pins.json"), join(scratch.path, "pins.json"));
  const alteredSchema = `${schema}\n`;
  const alteredPairing = pairing.replace(
    '"responseDefinition": "#/definitions/v2/ModelListResponse"',
    '"responseDefinition": "#/definitions/v2/ModelProviderCapabilitiesReadResponse"',
  );
  writeFileSync(join(scratch.path, "codex-app-server-0.166.0.experimental.json"), alteredSchema);
  writeFileSync(join(scratch.path, "codex-app-server-0.166.0.pairing.json"), alteredPairing);

  assert.throws(() => readPinnedCodexInput(scratch.path, "codexAppServer"), /SHA-256 mismatch/);
  assert.throws(
    () => readPinnedCodexInput(scratch.path, "codexAppServerPairing"),
    /SHA-256 mismatch/,
  );
  assert.throws(
    () => renderCodexBindings(schema, alteredPairing),
    /pairing pointers disagree with declared Rust types/,
  );
});

test("semantic validation refuses unsupported constraints and unresolved payload references", (): void => {
  const unsupported = schema.replace(
    '"title": "ModelListParams",',
    '"title": "ModelListParams", "minProperties": 1,',
  );
  const unresolved = schema.replace('"ModelUpgradeInfo": {', '"RenamedUpgradeInfo": {');

  assert.throws(
    () => renderCodexBindings(unsupported, pairing),
    /ModelListParams: unsupported definition.*minProperties/,
  );
  assert.throws(
    () => renderCodexBindings(unresolved, pairing),
    /ModelUpgradeInfo: missing definition/,
  );
});

const definitionFields = z.looseObject({ properties: z.record(z.string(), z.unknown()) });
const taggedBranches = z.looseObject({ oneOf: z.array(definitionFields) });

test("turn generation rejects overlapping enum branches and unsupported output constraints", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const summary = bundle.definitions.v2.ReasoningSummary;
  bundle.definitions.v2.ReasoningSummary = {
    oneOf: [
      { type: "string", enum: ["auto", "concise", "detailed"] },
      { type: "string", enum: ["none", "auto"] },
    ],
  };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /ReasoningSummary: overlapping string enum tags/,
  );
  bundle.definitions.v2.ReasoningSummary = summary;
  const request = definitionFields.parse(bundle.definitions.v2.TurnStartParams);
  bundle.definitions.v2.TurnStartParams = {
    ...request,
    properties: {
      ...request.properties,
      outputSchema: { minProperties: 1 },
    },
  };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /TurnStartParams\/properties\/outputSchema: unsupported wire value/,
  );
});

test("buffered union generation accepts the pinned granular approval branch", (): void => {
  const generated = renderCodexBindings(schema, pairing);
  assert.match(
    generated,
    /#\[serde\(untagged\)\]\npub enum AskForApproval \{\s+Known\(AskForApprovalTag\),\s+Granular\(GranularAskForApproval\),/,
  );
});

for (const [label, futurePayload, path] of [
  ["direct", true, "futurePayload"],
  [
    "transitive",
    { type: "array", items: { $ref: "#/definitions/v2/ThreadStatus" } },
    "futurePayload/items -> #/definitions/v2/ThreadStatus",
  ],
] as const) {
  test(`buffered union generation refuses a ${label} raw JSON decoder`, (): void => {
    const bundle = codexBundleSchema.parse(JSON.parse(schema));
    const approval = z
      .looseObject({ oneOf: z.tuple([z.unknown(), definitionFields]) })
      .parse(bundle.definitions.v2.AskForApproval);
    const [tags, branch] = approval.oneOf;
    const granular = definitionFields.parse(branch.properties.granular);
    bundle.definitions.v2.AskForApproval = {
      ...approval,
      oneOf: [
        tags,
        {
          ...branch,
          properties: {
            ...branch.properties,
            granular: {
              ...granular,
              properties: { ...granular.properties, futurePayload },
            },
          },
        },
      ],
    };
    assert.throws(() => renderCodexBindings(JSON.stringify(bundle), pairing), {
      message:
        `#/definitions/v2/AskForApproval/oneOf/1/properties/granular/properties/${path}: ` +
        "raw JSON decoder cannot run inside a buffered union",
    });
  });
}

test("thread binding generation refuses unknown composition and constraints at the affected field", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const fields = definitionFields.parse(bundle.definitions.v2.ThreadResumeResponse);
  for (const [field, replacement] of [
    [
      "approvalsReviewer",
      {
        allOf: [
          { $ref: "#/definitions/v2/ApprovalsReviewer" },
          { $ref: "#/definitions/v2/SandboxPolicy" },
        ],
      },
    ],
    ["runtimeWorkspaceRoots", { type: "array", items: true, minItems: 1 }],
  ] as const) {
    bundle.definitions.v2.ThreadResumeResponse = {
      ...fields,
      properties: { ...fields.properties, [field]: replacement },
    };
    assert.throws(
      () => renderCodexBindings(JSON.stringify(bundle), pairing),
      new RegExp(`ThreadResumeResponse/properties/${field}: unsupported wire value`),
    );
  }
});

test("thread binding generation refuses ambiguous discriminators and altered alternatives", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const status = taggedBranches.parse(bundle.definitions.v2.ThreadStatus);
  const [first, second, ...remaining] = status.oneOf;
  assert.ok(first && second);
  bundle.definitions.v2.ThreadStatus = {
    oneOf: [
      first,
      { ...second, properties: { ...second.properties, type: first.properties.type } },
      ...remaining,
    ],
  };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /ThreadStatus: duplicate tagged union discriminator/,
  );
  bundle.definitions.v2.ThreadStatus = status;
  const userInput = taggedBranches.parse(bundle.definitions.v2.UserInput);
  const alternatives = z.array(definitionFields);
  const branches = userInput.oneOf.map((branch) => {
    const parsed = alternatives.safeParse(branch.anyOf);
    if (!parsed.success) return branch;
    return {
      ...branch,
      anyOf: parsed.data.map((part) => ({
        ...part,
        properties: { ...part.properties, extra: { type: "integer" } },
      })),
    };
  });
  bundle.definitions.v2.UserInput = { oneOf: branches };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /UserInput\/oneOf\/1\/anyOf: expected separate required string alternatives/,
  );
});
