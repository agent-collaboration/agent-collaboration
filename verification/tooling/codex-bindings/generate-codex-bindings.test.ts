import assert from "node:assert/strict";
import {
  copyFileSync,
  existsSync,
  mkdtempDisposableSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import { codexBundleSchema } from "../schema-audit/codex-pairing-transcription.ts";
import { checkCodexBindings, writeCodexBindings } from "./codex-generated-files.ts";
import { generatedRustHeader, renderRustModules, rustLineCount } from "./codex-rust-modules.ts";
import { readPinnedCodexInput, renderCodexBindings } from "./generate-codex-bindings.ts";

const repository = fileURLToPath(new URL("../../../", import.meta.url));
const schemas = fileURLToPath(new URL("../../schemas/", import.meta.url));
const committed = fileURLToPath(
  new URL("../../../crates/vendor-protocol/src/codex/generated", import.meta.url),
);
const schema = readPinnedCodexInput(schemas, "codexAppServer");
const pairing = readPinnedCodexInput(schemas, "codexAppServerPairing");

test("two offline generations match the committed shared binding closure", (): void => {
  const generated = renderCodexBindings(schema, pairing);
  assert.deepEqual(renderCodexBindings(schema, pairing), generated);
  const source = [...generated.values()].join("\n");
  const definitions = source.match(/^\/\/ #\/definitions\/v2\/\w+$/gm);
  assert.equal(definitions?.length, 113);
  assert.equal(new Set(definitions).size, 113);
  assert.equal(source.match(/impl super::CodexRequest for /g)?.length, 4);
  assert.equal(source.match(/impl super::CodexServerNotification for /g)?.length, 5);
  for (const [name, module] of generated) assert.ok(rustLineCount(module) <= 1000, name);
  checkCodexBindings(committed, generated, `${committed}.rs`);
});

test("offline drift check rejects an edited secondary module and leaves its bytes intact", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-binding-"));
  const files = renderCodexBindings(schema, pairing);
  writeCodexBindings(scratch.path, files);
  const output = join(scratch.path, "pair_thread_read.rs");
  const edited = readFileSync(output, "utf8").replace('"thread/read"', '"thread/edited"');
  writeFileSync(output, edited);

  assert.throws(
    () => checkCodexBindings(scratch.path, files),
    /pair_thread_read\.rs: generated Codex bindings differ/,
  );
  assert.equal(readFileSync(output, "utf8"), edited);
});

test("offline drift check rejects both missing and unexpected modules", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-binding-set-"));
  const files = renderCodexBindings(schema, pairing);
  writeCodexBindings(scratch.path, files);
  const missing = join(scratch.path, "pair_thread_read.rs");
  unlinkSync(missing);
  assert.throws(
    () => checkCodexBindings(scratch.path, files),
    /pair_thread_read\.rs: missing generated Rust module/,
  );
  assert.equal(existsSync(missing), false);

  const unexpected = join(scratch.path, "incidental.rs");
  const incidental = "// incidental file\n";
  writeFileSync(unexpected, incidental);
  assert.throws(
    () => checkCodexBindings(scratch.path, files),
    /incidental\.rs: unexpected generated output/,
  );
  assert.throws(
    () => writeCodexBindings(scratch.path, files),
    /incidental\.rs: unexpected generated output/,
  );
  assert.equal(readFileSync(unexpected, "utf8"), incidental);
  assert.equal(existsSync(missing), false);
});

test("offline check rejects a 1001-line secondary module without rewriting it", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-binding-size-"));
  const files = renderCodexBindings(schema, pairing);
  writeCodexBindings(scratch.path, files);
  const output = join(scratch.path, "pair_thread_read.rs");
  const source = readFileSync(output, "utf8");
  const oversized = source + "// scratch line\n".repeat(1001 - rustLineCount(source));
  writeFileSync(output, oversized);
  assert.throws(
    () => checkCodexBindings(scratch.path, files),
    /pair_thread_read\.rs: generated Rust has 1001 lines, limit is 1000/,
  );
  assert.equal(readFileSync(output, "utf8"), oversized);
});

test("generation replaces owned monolithic and stale outputs with the bounded set", (): void => {
  using scratch = mkdtempDisposableSync(join(tmpdir(), "codex-legacy-binding-"));
  const directory = join(scratch.path, "generated");
  const legacy = join(scratch.path, "generated.rs");
  const files = renderCodexBindings(schema, pairing);
  const monolith = [...files.values()].join("\n");
  assert.ok(rustLineCount(monolith) > 1000);
  writeFileSync(legacy, monolith);
  assert.throws(
    () => checkCodexBindings(directory, files, legacy),
    /generated\.rs: generated Rust has \d+ lines, limit is 1000/,
  );
  assert.equal(readFileSync(legacy, "utf8"), monolith);

  writeCodexBindings(directory, files, legacy);
  assert.equal(existsSync(legacy), false);
  const stale = join(directory, "stale_generated.rs");
  writeFileSync(stale, `${generatedRustHeader}\n`);
  assert.throws(
    () => checkCodexBindings(directory, files),
    /stale_generated\.rs: unexpected generated output/,
  );
  writeCodexBindings(directory, files);
  assert.equal(existsSync(stale), false);
  checkCodexBindings(directory, files, legacy);
});

test("the formatted line budget splits declarations and rejects an indivisible item", (): void => {
  const declaration = (name: string, count: number) => ({
    name,
    exports: [name],
    references: [],
    needsSerde: false,
    source: `pub struct ${name} { ${Array.from({ length: count }, (_, index) => `field_${index}: String,`).join(" ")} }`,
  });
  const partitions = renderRustModules(
    {
      declarations: [declaration("FirstPayload", 600), declaration("SecondPayload", 600)],
      pairings: [],
    },
    repository,
  );
  assert.deepEqual([...partitions.keys()], ["first_payload.rs", "second_payload.rs", "mod.rs"]);
  for (const module of partitions.values()) assert.ok(rustLineCount(module) <= 1000);
  const paired = renderRustModules(
    {
      declarations: [declaration("Request", 600), declaration("Response", 600)],
      pairings: [
        {
          name: "pair_fixture",
          exports: [],
          references: ["Request", "Response"],
          needsSerde: false,
          source: "impl super::CodexRequest for Request { type Response = Response; }",
        },
      ],
    },
    repository,
  );
  assert.deepEqual([...paired.keys()], ["response.rs", "request.rs", "pair_fixture.rs", "mod.rs"]);
  assert.throws(
    () =>
      renderRustModules(
        { declarations: [declaration("OversizedPayload", 1001)], pairings: [] },
        repository,
      ),
    /oversized_payload\.rs: indivisible generated Rust has \d+ lines after rustfmt, limit is 1000/,
  );
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

const notificationBranches = z.looseObject({
  oneOf: z.array(
    z.looseObject({
      properties: z.looseObject({
        method: z.looseObject({ enum: z.tuple([z.string()]) }),
        params: z.unknown().optional(),
      }),
    }),
  ),
});

test("server notification generation rejects an absent or repeated method declaration", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const original = notificationBranches.parse(bundle.definitions.ServerNotification);
  const matches = original.oneOf.filter(
    (branch) => branch.properties.method.enum[0] === "turn/started",
  );
  assert.equal(matches.length, 1);

  bundle.definitions.ServerNotification = {
    ...original,
    oneOf: original.oneOf.filter((branch) => branch.properties.method.enum[0] !== "turn/started"),
  };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /turn\/started: expected exactly one ServerNotification envelope, found 0/,
  );
  bundle.definitions.ServerNotification = { ...original, oneOf: [...original.oneOf, ...matches] };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /turn\/started: expected exactly one ServerNotification envelope, found 2/,
  );
});

test("server notification generation rejects a shared params reference between equal-shaped events", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const original = notificationBranches.parse(bundle.definitions.ServerNotification);
  bundle.definitions.ServerNotification = {
    ...original,
    oneOf: original.oneOf.map((branch) => {
      if (branch.properties.method.enum[0] !== "turn/started") return branch;
      return {
        ...branch,
        properties: {
          ...branch.properties,
          params: { $ref: "#/definitions/v2/TurnCompletedNotification" },
        },
      };
    }),
  };
  assert.throws(
    () => renderCodexBindings(JSON.stringify(bundle), pairing),
    /ServerNotification methods turn\/started and turn\/completed share params #\/definitions\/v2\/TurnCompletedNotification/,
  );
});

test("server notification generation derives distinct payload references from the export", (): void => {
  const bundle = codexBundleSchema.parse(JSON.parse(schema));
  const original = notificationBranches.parse(bundle.definitions.ServerNotification);
  bundle.definitions.ServerNotification = {
    ...original,
    oneOf: original.oneOf.map((branch) => {
      const method = branch.properties.method.enum[0];
      if (method !== "turn/started" && method !== "turn/completed") return branch;
      const payload =
        method === "turn/started" ? "TurnCompletedNotification" : "TurnStartedNotification";
      return {
        ...branch,
        properties: {
          ...branch.properties,
          params: { $ref: `#/definitions/v2/${payload}` },
        },
      };
    }),
  };
  const generated = renderCodexBindings(JSON.stringify(bundle), pairing).get(
    "observation_events.rs",
  );
  assert.ok(generated);
  assert.match(
    generated,
    /impl super::CodexServerNotification for TurnCompletedNotification \{\s+const METHOD: &'static str = "turn\/started";/,
  );
  assert.match(
    generated,
    /impl super::CodexServerNotification for TurnStartedNotification \{\s+const METHOD: &'static str = "turn\/completed";/,
  );
});

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
  const generated = [...renderCodexBindings(schema, pairing).values()].join("\n");
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
