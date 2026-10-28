import { readFileSync } from "node:fs";
import { join } from "node:path";

export function functionBody(source: string, functionName: string): string {
  const escapedName = functionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`\\n\\s*(?:async\\s+)?fn\\s+${escapedName}\\b`).exec(source);
  if (match === null) throw new Error(`missing function ${functionName}`);
  const brace = source.indexOf("{", match.index);
  if (brace === -1) throw new Error(`missing function body for ${functionName}`);
  let depth = 0;
  for (let index = brace; index < source.length; index++) {
    if (source[index] === "{") depth++;
    else if (source[index] === "}") {
      depth--;
      if (depth === 0) return source.slice(brace, index + 1);
    }
  }
  throw new Error(`unterminated function body for ${functionName}`);
}

function requireContains(label: string, source: string, marker: string): void {
  if (!source.includes(marker)) throw new Error(`${label} missing required marker: ${marker}`);
}

function forbidContains(label: string, source: string, marker: string): void {
  if (source.includes(marker)) throw new Error(`${label} contains forbidden marker: ${marker}`);
}

export function checkHttpStreaming(
  row: "G-24" | "G-25",
  repositoryRoot: string = process.cwd(),
): void {
  const read = (name: string): string =>
    readFileSync(join(repositoryRoot, `crates/codex-router-proxy/src/${name}.rs`), "utf8");
  const server = read("server");
  if (row === "G-24") {
    const upstream = read("upstream");
    const httpSse = read("http_sse");
    const adapter = functionBody(server, "hyper_request_to_streaming_proxy_request");
    const prefix = functionBody(server, "bounded_request_metadata_body");
    const requestBody = functionBody(server, "request_metadata_prefix_is_complete_json");
    requireContains("HTTP request adapter", adapter, "bounded_request_metadata_body");
    for (const marker of [
      "HTTP_REQUEST_METADATA_PREFIX_MAX_BYTES",
      "body.frame().await",
      "VecDeque",
      "PrefixFramesThenIncomingBody::new",
    ])
      requireContains("HTTP request prefix helper", prefix, marker);
    requireContains("HTTP request complete helper", requestBody, "serde_json::from_slice");
    for (const marker of [".collect()", ".to_bytes()"])
      forbidContains("HTTP request adapter", adapter, marker);
    const upstreamSend = functionBody(upstream, "send_streaming_inner");
    requireContains("Hyper upstream transport", upstream, "request: StreamingUpstreamHttpRequest");
    requireContains("Hyper upstream transport", upstreamSend, ".body(request.into_body())");
    for (const marker of [
      "Full::new",
      "copy_from_slice(request.body())",
      "request.body().to_vec()",
    ])
      forbidContains("Hyper upstream transport", upstreamSend, marker);
    requireContains("streaming request DTO", httpSse, "pub struct StreamingUpstreamHttpRequest");
    requireContains(
      "streaming async transport trait",
      httpSse,
      "request: StreamingUpstreamHttpRequest",
    );
    return;
  }
  const websocket = read("websocket");
  for (const marker of [
    "HTTP_RESPONSE_AFFINITY_SCAN_MAX_BYTES",
    "HTTP_RESPONSE_AFFINITY_SCAN_MAX_EVENTS",
  ])
    requireContains("HTTP/SSE affinity constants", server, marker);
  const affinityTap = functionBody(server, "record_affinity_owner_from_async_body");
  for (const marker of [
    "scanned_bytes",
    "scanned_events",
    "HTTP_RESPONSE_AFFINITY_SCAN_MAX_BYTES",
    "HTTP_RESPONSE_AFFINITY_SCAN_MAX_EVENTS",
    "data[..bytes_to_scan]",
  ])
    requireContains("HTTP/SSE affinity tap", affinityTap, marker);
  forbidContains("HTTP/SSE affinity tap", affinityTap, "buffered.extend_from_slice(data)");
  const pump = functionBody(websocket, "pump_upstream_to_local");
  for (const marker of [
    "websocket_metadata_text_handle",
    "local_write.send(upstream_message).await?",
    "record_forwarded_websocket_metadata",
  ])
    requireContains("WebSocket pump", pump, marker);
  const sendIndex = pump.indexOf("local_write.send(upstream_message).await?");
  if (sendIndex > pump.indexOf("record_forwarded_websocket_metadata"))
    throw new Error("WebSocket pump records metadata before forwarding local frame");
  for (const marker of [
    "serde_json::from_str",
    "is_response_completed_text",
    "websocket_affinity_owner_record_from_text",
  ])
    forbidContains("WebSocket pre-send pump", pump.slice(0, sendIndex), marker);
  const metadata = functionBody(websocket, "websocket_metadata_text_handle");
  for (const marker of ["Message::Text", "text.clone()"])
    requireContains("WebSocket metadata helper", metadata, marker);
  forbidContains("WebSocket metadata helper", metadata, ".to_string()");
  const recorder = functionBody(websocket, "record_forwarded_websocket_metadata");
  for (const marker of ["is_response_completed_text", "websocket_affinity_owner_record_from_text"])
    requireContains("WebSocket metadata recorder", recorder, marker);
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  const [row] = arguments_;
  if (arguments_.length !== 1 || (row !== "G-24" && row !== "G-25")) {
    console.error("usage: node verification/tooling/check-http-streaming-guard.ts G-24|G-25");
    return 2;
  }
  try {
    checkHttpStreaming(row);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }
  console.log(`${row} HTTP/SSE streaming guard passed`);
  return 0;
}

if (import.meta.main) process.exitCode = main();
