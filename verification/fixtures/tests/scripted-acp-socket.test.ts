import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";

for (const action of ["wait_for_socket_signal", "exit_on_socket_signal"]) {
  test(
    `scripted ACP ${action} is released by its actual Unix socket`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const root = createFixtureDirectory("acp-socket");
      const socketPath = join(root, "gate.sock");
      const server = createServer((connection): void => {
        connection.end("x");
      });
      context.after(() => {
        server.close();
        rmSync(root, { recursive: true, force: true });
      });
      server.listen(socketPath);
      await once(server, "listening");
      const steps = [
        { action, socketPath },
        { action: "send", message: { released: true } },
        { action: "exit" },
      ];
      const child = spawn(
        process.execPath,
        [new URL("../scripted-acp-agent.ts", import.meta.url).pathname, JSON.stringify(steps)],
        { stdio: ["pipe", "pipe", "pipe"] },
      );
      context.after(() => {
        if (child.exitCode === null && child.signalCode === null) child.kill();
      });
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (bytes: Buffer): void => {
        stdout += bytes.toString("utf8");
      });
      child.stderr.on("data", (bytes: Buffer): void => {
        stderr += bytes.toString("utf8");
      });
      const [status] = await once(child, "close");
      assert.equal(status, 0, stderr);
      assert.equal(stdout, action === "wait_for_socket_signal" ? '{"released": true}\n' : "");
    },
  );
}

test(
  "scripted ACP rejects a request arriving before the socket releases settlement",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const root = createFixtureDirectory("acp-socket-reject");
    const socketPath = join(root, "gate.sock");
    const server = createServer();
    context.after(() => {
      server.close();
      rmSync(root, { recursive: true, force: true });
    });
    server.listen(socketPath);
    await once(server, "listening");
    const child = spawn(
      process.execPath,
      [
        new URL("../scripted-acp-agent.ts", import.meta.url).pathname,
        JSON.stringify([{ action: "wait_for_socket_signal", socketPath }]),
      ],
      { stdio: ["pipe", "pipe", "pipe"] },
    );
    context.after(() => {
      if (child.exitCode === null && child.signalCode === null) child.kill();
    });
    server.once("connection", (connection): void => {
      context.after(() => {
        connection.destroy();
      });
      child.stdin.write('{"jsonrpc":"2.0","id":1,"method":"premature","params":{}}\n');
    });
    let stderr = "";
    child.stderr.on("data", (bytes: Buffer): void => {
      stderr += bytes.toString("utf8");
    });
    const [status] = await once(child, "close");
    assert.equal(status, 1);
    assert.ok(stderr.includes("no provider request before turn settlement"));
    assert.ok(stderr.includes("premature"));
  },
);
