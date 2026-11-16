import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const recorder = join(import.meta.dirname, "acp-stdio-recorder.mjs");

/** A stand-in agent: answers each JSON-RPC request line on stdout, after printing `banner`. */
const AGENT = `
import { createInterface } from "node:readline";
if (process.argv[2] === "--version") { process.stdout.write("0.81.0\\n"); process.exit(0); }
const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  const request = JSON.parse(line);
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result: { echoed: request.method } }) + "\\n");
});
`;

function runAgent(
  input: string,
  args: string[] = [],
  source: string = AGENT,
): { directory: string; stdout: string } {
  const scripts = mkdtempSync(join(tmpdir(), "d0-acp-agent-"));
  const agent = join(scripts, "agent.mjs");
  writeFileSync(agent, source);
  const directory = mkdtempSync(join(tmpdir(), "d0-acp-recording-"));
  const result = spawnSync(process.execPath, [agent, ...args], {
    input,
    encoding: "utf8",
    env: {
      ...process.env,
      NODE_OPTIONS: `--import ${recorder}`,
      CODEX_ROUTER_D0_ACP_RECORDING_DIR: directory,
    },
  });
  assert.equal(result.status, 0, result.stderr);
  return { directory, stdout: result.stdout };
}

function recordingLines(directory: string): Array<Record<string, unknown>> {
  const files = readdirSync(directory).filter((file) => file.endsWith(".jsonl"));
  assert.equal(files.length, 1, `one recording, got ${files.join(",")}`);
  const file = files[0];
  assert.ok(file !== undefined);
  return readFileSync(join(directory, file), "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

test("every ACP line is recorded in order, both directions, unchanged for the agent", () => {
  const requests = [
    { jsonrpc: "2.0", id: 0, method: "initialize", params: { protocolVersion: 1 } },
    { jsonrpc: "2.0", id: 1, method: "session/new", params: { cwd: "/tmp", mcpServers: [] } },
  ];
  const { directory, stdout } = runAgent(
    requests.map((request) => JSON.stringify(request)).join("\n") + "\n",
  );
  const lines = recordingLines(directory);
  assert.deepEqual(
    lines.map((line) => [line["seq"], line["direction"], (line["message"] as { id: number }).id]),
    // All input arrives at once, so both requests are read before either reply is written.
    [
      [0, "clientToServer", 0],
      [1, "clientToServer", 1],
      [2, "serverToClient", 0],
      [3, "serverToClient", 1],
    ],
  );
  assert.deepEqual(lines[0]?.["message"], requests[0]);
  assert.equal(stdout.trim().split("\n").length, 2, "the agent's own output is unchanged");
});

test("the qualification probe, which speaks no ACP, writes nothing", () => {
  const { directory, stdout } = runAgent("", ["--version"]);
  assert.equal(stdout, "0.81.0\n");
  assert.deepEqual(readdirSync(directory), []);
});

test("an MCP server under the same environment is never recorded as ACP", () => {
  const mcpInitialize = {
    jsonrpc: "2.0",
    id: 0,
    method: "initialize",
    params: {
      protocolVersion: "2025-11-25",
      capabilities: {},
      clientInfo: { name: "c", version: "1" },
    },
  };
  const { directory } = runAgent(`${JSON.stringify(mcpInitialize)}\n`);
  assert.deepEqual(readdirSync(directory), []);
});

const INITIALIZE = `${JSON.stringify({ jsonrpc: "2.0", id: 0, method: "initialize", params: { protocolVersion: 1 } })}\n`;

test("a UTF-8 character split across two writes is recorded whole", () => {
  // The reply's text holds "é" (0xC3 0xA9); the agent writes the bytes in two chunks, split inside it.
  const splitWriter = `
process.stdin.once("data", () => {
  const bytes = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id: 0, result: { text: "café" } }) + "\\n", "utf8");
  const split = bytes.indexOf(0xa9);
  process.stdout.write(bytes.subarray(0, split));
  process.stdout.write(bytes.subarray(split));
});
`;
  const { directory, stdout } = runAgent(INITIALIZE, [], splitWriter);
  assert.ok(stdout.includes("café"));
  const reply = recordingLines(directory).find((line) => line["direction"] === "serverToClient");
  assert.deepEqual(reply?.["message"], { jsonrpc: "2.0", id: 0, result: { text: "café" } });
});

test("a final line left without its newline is kept as an unparsed fragment, not dropped", () => {
  const unterminated = `
process.stdin.once("data", () => {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: 0, result: {} }).slice(0, 20));
});
`;
  const { directory } = runAgent(INITIALIZE, [], unterminated);
  const last = recordingLines(directory).at(-1);
  assert.equal(last?.["direction"], "serverToClient");
  assert.equal(last?.["unterminated"], true);
  assert.equal(typeof last?.["unparsed"], "string");
});
