import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { z } from "zod";

import {
  codexBundleSchema,
  codexPairingSchema,
} from "../schema-audit/codex-pairing-transcription.ts";
import { checkCodexBindings, writeCodexBindings } from "./codex-generated-files.ts";
import { renderRustBindings } from "./codex-rust-binding.ts";
import {
  groupNotificationItems,
  resolveNotifications,
  selectedNotifications,
} from "./codex-notifications.ts";
import { renderRustModules } from "./codex-rust-modules.ts";
import { definitionName, definitionRef, reachableDefinitions } from "./codex-schema.ts";

const repository = fileURLToPath(new URL("../../../", import.meta.url));
const schemaDirectory = join(repository, "verification/schemas");
const committedBindings = join(repository, "crates/vendor-protocol/src/codex/generated");
const legacyBindings = `${committedBindings}.rs`;
const selectedMethods = ["model/list", "thread/read", "thread/resume", "turn/start"];

const pinSchema = z.object({
  protocol: z.string(),
  file: z.string().regex(/^[a-zA-Z0-9][a-zA-Z0-9.-]*\.json$/),
  sha256: z.string().regex(/^[a-f0-9]{64}$/),
});
const pinsSchema = z.object({ pins: z.array(pinSchema) });

export function readPinnedCodexInput(
  directory: string,
  protocol: "codexAppServer" | "codexAppServerPairing",
): string {
  const { pins } = pinsSchema.parse(JSON.parse(readFileSync(join(directory, "pins.json"), "utf8")));
  const matches = pins.filter((pin) => pin.protocol === protocol);
  const [pin] = matches;
  if (matches.length !== 1 || pin === undefined) throw new Error(`${protocol}: expected one pin`);
  const bytes = readFileSync(join(directory, pin.file));
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  if (sha256 !== pin.sha256) throw new Error(`${pin.file}: SHA-256 mismatch for ${protocol}`);
  return bytes.toString("utf8");
}

function resolveBinding(
  bundle: z.infer<typeof codexBundleSchema>,
  pairing: z.infer<typeof codexPairingSchema>,
  selectedMethod: string,
): { method: string; params: string; response: string } {
  const selected = pairing.clientRequests.filter((entry) => entry.method === selectedMethod);
  const [request] = selected;
  if (selected.length !== 1 || request?.bundle.kind !== "exported") {
    throw new Error(`${selectedMethod}: expected one exported pairing`);
  }

  const params = definitionRef.parse(request.bundle.paramsDefinition);
  const response = definitionRef.parse(request.bundle.responseDefinition);
  if (
    request.paramsType !== `v2::${definitionName(params)}` ||
    request.responseType !== `v2::${definitionName(response)}` ||
    params === response
  ) {
    throw new Error(`${selectedMethod}: pairing pointers disagree with declared Rust types`);
  }
  const envelopes = bundle.definitions.ClientRequest.oneOf.filter(
    (entry) => entry.properties.method.enum[0] === request.method,
  );
  const [envelope] = envelopes;
  const reference = z.strictObject({ $ref: definitionRef }).safeParse(envelope?.properties.params);
  if (envelopes.length !== 1 || !reference.success || reference.data.$ref !== params) {
    throw new Error(`${selectedMethod}: pairing params disagree with the exported request`);
  }

  return { method: request.method, params, response };
}

export function renderCodexBindings(
  schemaText: string,
  pairingText: string,
): ReadonlyMap<string, string> {
  const bundle = codexBundleSchema.parse(JSON.parse(schemaText));
  const pairing = codexPairingSchema.parse(JSON.parse(pairingText));
  const bindings = selectedMethods.map((method) => resolveBinding(bundle, pairing, method));
  const notifications = resolveNotifications(bundle.definitions.ServerNotification);
  const roots = [
    ...bindings.flatMap(({ params, response }) => [params, response]),
    ...notifications.map(({ params }) => params),
  ];
  const definitions = reachableDefinitions(bundle.definitions.v2, roots);
  for (const root of roots) {
    if (definitions.get(root)?.definition.kind !== "object") {
      throw new Error(`${root}: expected an object payload`);
    }
  }

  const rendered = renderRustBindings(bindings, definitions);
  const groups = groupNotificationItems(rendered.declarations, notifications);
  return renderRustModules({ ...groups, pairings: rendered.pairings }, repository);
}

function main(args: readonly string[]): void {
  const [mode, ...rest] = args;
  if (rest.length !== 0 || (mode !== "--write" && mode !== "--check")) {
    throw new Error("usage: generate-codex-bindings.ts --write | --check");
  }
  const schema = readPinnedCodexInput(schemaDirectory, "codexAppServer");
  const pairing = readPinnedCodexInput(schemaDirectory, "codexAppServerPairing");
  const generated = renderCodexBindings(schema, pairing);
  if (mode === "--write") {
    writeCodexBindings(committedBindings, generated, legacyBindings);
  } else {
    checkCodexBindings(committedBindings, generated, legacyBindings);
  }
  const methods = [...selectedMethods, ...selectedNotifications];
  console.log(
    `Codex bindings ${mode === "--write" ? "generated" : "match"}: ${methods.join(", ")}`,
  );
}

if (import.meta.main) main(process.argv.slice(2));
