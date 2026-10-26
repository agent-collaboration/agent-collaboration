import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { once } from "node:events";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { decode } from "../exact-json.ts";
import { evidenceJson, prettyEvidenceJson } from "../evidence-json.ts";
import {
  harnessClaimsSchema,
  hostManifestSchema,
  readObservedJson,
  routerStartedSchema,
} from "./evidence-models.ts";

function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#x27;");
}

export function reportHtml(evidence: string): Buffer {
  const claims = harnessClaimsSchema.parse(
    decode(readFileSync(join(evidence, "claims.json"), "utf8")),
  );
  const manifest = hostManifestSchema.parse(
    decode(readFileSync(join(evidence, "host-A-ready.json"), "utf8")),
  );
  const started = routerStartedSchema.parse(
    decode(readFileSync(join(evidence, "router-started.json"), "utf8")),
  );
  const sections = Object.entries(claims.details)
    .map(
      ([title, path]) =>
        `<section><h2>${escapeHtml(title)}</h2><pre>${escapeHtml(prettyEvidenceJson(readObservedJson(readFileSync(join(evidence, path)))))}</pre></section>`,
    )
    .join("");
  const rows = claims.rows
    .map((row) => `<tr>${row.map((value) => `<td>${escapeHtml(value)}</td>`).join("")}</tr>`)
    .join("");
  return Buffer.from(`<!doctype html><html><meta charset="utf-8"><title>Codex Router verification harness</title>
<style>body{font:17px system-ui;margin:36px;max-width:1180px;color:#17212b;background:white}h1{font-size:30px}header{border-bottom:3px solid #17212b;padding-bottom:20px}table{border-collapse:collapse;width:100%;margin:24px 0}td,th{padding:12px;text-align:left;border-bottom:1px solid #bbc4ce}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:13px;background:#edf3fa;padding:18px}section{margin-top:30px}.gap{color:#9b3019}</style>
<header><h1>Codex Router — actual verification evidence</h1><p>HARNESS UI SCREENSHOT · ${escapeHtml(manifest.machineLabel)} · not a native app screenshot · no delivery acceptance</p><p>Build ${escapeHtml(manifest.serviceVersion)} / Router ${escapeHtml(started.binarySHA256.slice(0, 12))} · source ${escapeHtml(started.head.slice(0, 12))}<br>Host A ${escapeHtml(manifest.serviceId)} · epoch ${escapeHtml(manifest.serviceEpoch)}</p></header>
<table><tr><th>Claim</th><th>Observation</th><th>Conclusion</th></tr>${rows}</table>
<p>${escapeHtml(claims.scope)}</p>${sections}</html>`);
}

export async function main(): Promise<void> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log("usage: harness-report.ts EVIDENCE [--port PORT]");
    return;
  }
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: { port: { type: "string", default: "0" } },
  });
  const evidence = positionals[0];
  if (positionals.length !== 1 || evidence === undefined)
    throw new Error("usage: harness-report.ts EVIDENCE [--port PORT]");
  const port = Number(values.port);
  if (!Number.isInteger(port)) throw new Error("report port must be an integer");
  const content = reportHtml(evidence);
  writeFileSync(join(evidence, "harness-report.html"), content);
  const server = createServer((request, response): void => {
    if (request.method !== "GET") {
      response.writeHead(501);
      response.end();
      return;
    }
    if (request.url !== "/") {
      response.writeHead(404);
      response.end();
      return;
    }
    response.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Content-Length": String(content.length),
    });
    response.end(content);
  });
  server.listen(port, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string")
    throw new Error("report listener has no TCP port");
  console.log(
    evidenceJson({ url: `http://127.0.0.1:${address.port}/`, label: "harness UI, not native app" }),
  );
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
