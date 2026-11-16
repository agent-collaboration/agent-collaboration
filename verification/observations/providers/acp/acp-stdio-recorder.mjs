/**
 * ACP stdio recorder (D0 Program Design Y3, Claude over ACP): a Node preload loaded into the
 * Claude agent process. Set on the debug Host's own process environment,
 *   NODE_OPTIONS="--import <this file>"  CODEX_ROUTER_D0_ACP_RECORDING_DIR=<absolute dir>
 * because provider launches inherit it: the Host's command-line launch has an empty environment
 * (crates/codex-router-host/src/collaboration_runtime.rs:85) and spawns only add variables
 * (crates/acp-client-runtime/src/agent_session_client/provider_client_contract.rs:36).
 * Qualification replaces only the executable and arguments and keeps the environment
 * (crates/acp-provider-adapters/src/claude_launch.rs:62-63, :148), so the qualified wrapper still
 * runs with this recorder inside it.
 *
 * It records every line the agent reads on stdin (`clientToServer`) and writes on stdout
 * (`serverToClient`), in one order, as `{seq, connection, direction, message}` JSON lines in
 * `<dir>/acp-<pid>.jsonl` — the format `check_recording` reads. It writes nothing until it reads
 * an ACP `initialize` request on stdin (JSON-RPC 2.0, with a numeric `protocolVersion`): the
 * qualification probe runs Node with the same environment and speaks no ACP, and the Node
 * children Claude Code starts inherit it too, such as MCP servers, whose `initialize` carries a
 * date-string `protocolVersion`. A line
 * that is not JSON after recording started is written as `{seq, connection, direction, unparsed}`,
 * which `check_recording` rejects as corrupt rather than silently skipping it.
 *
 * It never changes what the agent reads or writes; a recording failure is written to
 * `<dir>/acp-<pid>.failure` and the agent carries on.
 */
import { appendFileSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { StringDecoder } from "node:string_decoder";

const directory = process.env.CODEX_ROUTER_D0_ACP_RECORDING_DIR;

if (directory !== undefined && directory !== "") {
  if (!isAbsolute(directory)) throw new Error("CODEX_ROUTER_D0_ACP_RECORDING_DIR must be absolute");
  installRecorder(directory);
}

function installRecorder(recordingDirectory) {
  const path = join(recordingDirectory, `acp-${process.pid}.jsonl`);
  const connection = `agent-${process.pid}`;
  const pending = { clientToServer: "", serverToClient: "" };
  // One decoder per direction, so a UTF-8 character split across two chunks decodes whole.
  const decoders = {
    clientToServer: new StringDecoder("utf8"),
    serverToClient: new StringDecoder("utf8"),
  };
  let seq = 0;
  let started = false;
  let failed = false;

  const fail = (error) => {
    if (failed) return;
    failed = true;
    try {
      writeFileSync(`${path.slice(0, -".jsonl".length)}.failure`, String(error?.stack ?? error));
    } catch {
      // The agent must keep running even when the recording cannot be kept.
    }
  };

  const recordLine = (direction, text) => {
    if (text.trim() === "") return;
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      message = undefined;
    }
    if (!started) {
      const acpInitialize =
        direction === "clientToServer" &&
        message?.jsonrpc === "2.0" &&
        message.method === "initialize" &&
        typeof message.params?.protocolVersion === "number";
      if (!acpInitialize) return;
      started = true;
    }
    const line =
      message === undefined
        ? { seq, connection, direction, unparsed: text }
        : { seq, connection, direction, message };
    seq += 1;
    appendFileSync(path, `${JSON.stringify(line)}\n`);
  };

  const take = (direction, chunk) => {
    if (failed || chunk === null || chunk === undefined) return;
    try {
      const text =
        typeof chunk === "string"
          ? chunk
          : decoders[direction].write(
              Buffer.from(chunk.buffer ?? chunk, chunk.byteOffset ?? 0, chunk.byteLength),
            );
      const lines = (pending[direction] + text).split("\n");
      pending[direction] = lines.pop() ?? "";
      for (const line of lines) recordLine(direction, line);
    } catch (error) {
      fail(error);
    }
  };

  // A fragment left without its newline when the process ends is not dropped: it is written as
  // an unparsed line, so check_recording rejects the capture instead of seeing a clean prefix.
  process.on("exit", () => {
    if (failed || !started) return;
    try {
      for (const direction of ["clientToServer", "serverToClient"]) {
        const rest = pending[direction] + decoders[direction].end();
        pending[direction] = "";
        if (rest.trim() !== "") {
          appendFileSync(
            path,
            `${JSON.stringify({ seq, connection, direction, unparsed: rest, unterminated: true })}\n`,
          );
          seq += 1;
        }
      }
    } catch (error) {
      fail(error);
    }
  });

  const stdin = process.stdin;
  const push = stdin.push.bind(stdin);
  stdin.push = (chunk, encoding) => {
    take("clientToServer", chunk);
    return push(chunk, encoding);
  };

  const write = process.stdout.write.bind(process.stdout);
  process.stdout.write = (chunk, encoding, callback) => {
    take("serverToClient", chunk);
    return write(chunk, encoding, callback);
  };
}
