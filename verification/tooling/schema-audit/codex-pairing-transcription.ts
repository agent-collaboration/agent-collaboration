/**
 * `node verification/tooling/schema-audit/codex-pairing-transcription.ts <common.rs> <bundle.json> <out.json>`
 *
 * Transcribes which response answers which request from Codex's own declaration (R50, Y1). The
 * exported JSON bundle carries no pairing, so the pairing is read from the
 * `client_request_definitions!` and `server_request_definitions!` invocations in
 * `codex-rs/app-server-protocol/src/protocol/common.rs` at the pinned release tag, and each
 * declared Rust type is resolved to its definition in the exported bundle. A type the bundle does
 * not define fails the transcription; it never falls back to a guess.
 *
 * Reproduce the pinned file with:
 *   git -C <codex checkout> show rust-v0.166.0:codex-rs/app-server-protocol/src/protocol/common.rs > common.rs
 *   codex app-server generate-json-schema --experimental --out <dir>
 *   node verification/tooling/schema-audit/codex-pairing-transcription.ts common.rs <dir>/codex_app_server_protocol.schemas.json out.json
 */
import { readFileSync, writeFileSync } from "node:fs";

import { z } from "zod";

/** A request family: requests the CLI sends to Codex, or requests Codex sends to the CLI. */
export type RequestSide = "client" | "server";

export interface PairedRequest {
  method: string;
  variant: string;
  experimental: boolean;
  /** The Rust type, as `common.rs` declares it (attributes removed). */
  paramsType: string;
  responseType: string;
  bundle: BundleDefinitions;
}

/**
 * Where the exported bundle defines the request. Codex declares a few deprecated requests that its
 * JSON export leaves out (`EXCLUDED_CLIENT_METHODS_FOR_JSON`, `export.rs:76` at the tag); those
 * are kept here as declared but not exported, with no definition.
 */
export type BundleDefinitions =
  | {
      kind: "exported";
      /** JSON pointer into the bundle; null when the request takes no params (`Option<()>`). */
      paramsDefinition: string | null;
      responseDefinition: string;
    }
  | { kind: "notExported" };

export interface CodexPairing {
  source: { repository: string; tag: string; path: string };
  clientRequests: PairedRequest[];
  serverRequests: PairedRequest[];
}

const requestUnionSchema = z.looseObject({
  oneOf: z.array(
    z.looseObject({
      properties: z.looseObject({ method: z.looseObject({ enum: z.tuple([z.string()]) }) }),
    }),
  ),
});
const bundleSchema = z.looseObject({
  definitions: z.looseObject({
    v2: z.record(z.string(), z.unknown()),
    ClientRequest: requestUnionSchema,
    ServerRequest: requestUnionSchema,
  }),
});

type Bundle = z.infer<typeof bundleSchema>;

const MACRO_OPENINGS: Record<RequestSide, string> = {
  client: "client_request_definitions! {",
  server: "server_request_definitions! {",
};

/** The body of one macro invocation, from its opening brace to the matching close. */
function invocationBody(source: string, side: RequestSide): string {
  const opening = MACRO_OPENINGS[side];
  const start = source.indexOf(`\n${opening}`);
  if (start < 0) {
    throw new Error(`common.rs has no invocation ${opening}`);
  }
  let depth = 0;
  const bodyStart = start + 1 + opening.length;
  for (let index = bodyStart - 1; index < source.length; index += 1) {
    const character = source[index];
    if (character === "{") depth += 1;
    if (character === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(bodyStart, index);
    }
  }
  throw new Error(`unterminated invocation ${opening}`);
}

/** Removes `#[...]` attributes (which may nest brackets) and returns the bare type. */
function bareType(declared: string): string {
  let text = declared.trim();
  while (text.startsWith("#[")) {
    let depth = 0;
    let end = 1;
    for (; end < text.length; end += 1) {
      if (text[end] === "[") depth += 1;
      if (text[end] === "]") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    text = text.slice(end + 1).trim();
  }
  return text;
}

function camelCase(variant: string): string {
  return variant.charAt(0).toLowerCase() + variant.slice(1);
}

/** Resolves `v1::X`, `v2::X` or a bare `X` to the bundle definition that carries it. */
function resolveDefinition(bundle: Bundle, rustType: string): string {
  const name = rustType.split("::").pop() ?? rustType;
  if (rustType.startsWith("v2::") && name in bundle.definitions.v2) {
    return `#/definitions/v2/${name}`;
  }
  if (name !== "v2" && name in bundle.definitions) {
    return `#/definitions/${name}`;
  }
  if (name in bundle.definitions.v2) {
    return `#/definitions/v2/${name}`;
  }
  throw new Error(`the bundle defines no ${rustType}`);
}

const ENTRY =
  /(#\[experimental\([^)]*\)\]\s*)?(?:(?:\/\/\/[^\n]*|\/\/[^\n]*)\s*)*([A-Z][A-Za-z0-9]*)\s*(?:=>\s*"([^"]+)")?\s*\{([^{}]*)\}/g;

export function transcribeRequests(
  source: string,
  bundle: Bundle,
  side: RequestSide,
): PairedRequest[] {
  const body = invocationBody(source, side);
  const union =
    side === "client" ? bundle.definitions.ClientRequest : bundle.definitions.ServerRequest;
  const exportedMethods = new Set(union.oneOf.map((entry) => entry.properties.method.enum[0]));
  const requests: PairedRequest[] = [];
  for (const match of body.matchAll(ENTRY)) {
    const [, experimental, variant, wire, rawFields] = match;
    if (variant === undefined || rawFields === undefined) continue;
    const fields = rawFields.replace(/^\s*\/\/.*$/gm, "");
    const params = /params:\s*([\s\S]*?),\s*\n\s*(?:inspect_params|serialization|response):/.exec(
      fields,
    );
    const response = /response:\s*([^,\n]+),/.exec(fields);
    if (params?.[1] === undefined || response?.[1] === undefined) {
      throw new Error(`${side} request ${variant} has no params or response`);
    }
    const paramsType = bareType(params[1]);
    const responseType = bareType(response[1]);
    const method = wire ?? camelCase(variant);
    requests.push({
      method,
      variant,
      experimental: experimental !== undefined,
      paramsType,
      responseType,
      bundle: exportedMethods.has(method)
        ? {
            kind: "exported",
            paramsDefinition:
              paramsType === "Option<()>" ? null : resolveDefinition(bundle, paramsType),
            responseDefinition: resolveDefinition(bundle, responseType),
          }
        : { kind: "notExported" },
    });
  }
  const declared = new Set(requests.map((request) => request.method));
  const undeclared = [...exportedMethods].filter((method) => !declared.has(method));
  if (undeclared.length > 0) {
    throw new Error(
      `the bundle exports ${side} requests common.rs does not declare: ${undeclared.join(", ")}`,
    );
  }
  return requests;
}

export function transcribePairing(source: string, bundleText: string): CodexPairing {
  const bundle = bundleSchema.parse(JSON.parse(bundleText));
  return {
    source: {
      repository: "https://github.com/openai/codex",
      tag: "rust-v0.166.0",
      path: "codex-rs/app-server-protocol/src/protocol/common.rs",
    },
    clientRequests: transcribeRequests(source, bundle, "client"),
    serverRequests: transcribeRequests(source, bundle, "server"),
  };
}

if (import.meta.main) {
  const [commonPath, bundlePath, outPath] = process.argv.slice(2);
  if (commonPath === undefined || bundlePath === undefined || outPath === undefined) {
    throw new Error("usage: codex-pairing-transcription.ts <common.rs> <bundle.json> <out.json>");
  }
  const pairing = transcribePairing(
    readFileSync(commonPath, "utf8"),
    readFileSync(bundlePath, "utf8"),
  );
  writeFileSync(outPath, `${JSON.stringify(pairing, null, 2)}\n`);
  console.log(
    `${pairing.clientRequests.length} client requests, ${pairing.serverRequests.length} server requests`,
  );
}
