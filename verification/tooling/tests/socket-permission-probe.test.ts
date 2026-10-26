import assert from "node:assert/strict";
import { chmodSync, mkdirSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { once } from "node:events";
import { test } from "node:test";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { connectionDenied } from "../socket-permission-probe.ts";

test("permission probes classify only access denial, never connection refusal or absence", (): void => {
  for (const code of ["EPERM", "EACCES"])
    assert.equal(connectionDenied(Object.assign(new Error("controlled failure"), { code })), true);
  for (const code of ["ENOENT", "ECONNREFUSED", "ETIMEDOUT"])
    assert.equal(connectionDenied(Object.assign(new Error("controlled failure"), { code })), false);
});

test("a live owned socket the caller can connect to cannot produce a denied-boundary receipt", async (context): Promise<void> => {
  const root = createFixtureDirectory("socket-probe-allowed");
  const path = join(root, "probe.sock");
  const server = createServer((connection): void => {
    connection.destroy();
  });
  context.after(() => {
    server.close();
    rmSync(root, { recursive: true, force: true });
  });
  server.listen(path);
  await once(server, "listening");
  const result = spawnSync(
    process.execPath,
    [new URL("../socket-permission-probe.ts", import.meta.url).pathname, path, "1"],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /Sandbox unexpectedly allowed SOCKET_BOUNDARY_OK/);
  const narrow = spawnSync(
    process.execPath,
    [
      new URL("../../fixtures/narrow-access-probe.ts", import.meta.url).pathname,
      path,
      "1",
      "NARROW_ACCESS_DENIED",
    ],
    { encoding: "utf8" },
  );
  assert.equal(narrow.status, 1);
  assert.equal(narrow.stdout, "");
  assert.match(narrow.stderr, /Sandbox unexpectedly allowed NARROW_ACCESS_DENIED/);
  const allowed = spawnSync(
    process.execPath,
    [new URL("../../fixtures/unix-access-probe.ts", import.meta.url).pathname, path],
    { encoding: "utf8" },
  );
  assert.equal(allowed.status, 0, allowed.stderr);
  assert.equal(allowed.stdout, "");
});

test("real Unix access denial advances to the TCP probe while an allowed TCP connection still refuses", async (context): Promise<void> => {
  const root = createFixtureDirectory("socket-probe-denied");
  const protectedDirectory = join(root, "protected");
  mkdirSync(protectedDirectory, { mode: 0o700 });
  const socket = createServer((connection): void => {
    connection.destroy();
  });
  const tcp = createServer((connection): void => {
    connection.destroy();
  });
  context.after(() => {
    chmodSync(protectedDirectory, 0o700);
    socket.close();
    tcp.close();
    rmSync(root, { recursive: true, force: true });
  });
  const path = join(protectedDirectory, "probe.sock");
  socket.listen(path);
  await once(socket, "listening");
  tcp.listen(0, "127.0.0.1");
  await once(tcp, "listening");
  const address = tcp.address();
  if (address === null || typeof address === "string")
    throw new Error("owned TCP port unavailable");
  chmodSync(protectedDirectory, 0o000);
  const result = spawnSync(
    process.execPath,
    [
      new URL("../socket-permission-probe.ts", import.meta.url).pathname,
      path,
      String(address.port),
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "SOCKET_BOUNDARY_OK\n");
  assert.match(result.stderr, /Sandbox unexpectedly allowed TCP_BOUNDARY_OK/);
  const denied = spawnSync(
    process.execPath,
    [new URL("../../fixtures/unix-access-probe.ts", import.meta.url).pathname, path],
    { encoding: "utf8" },
  );
  assert.equal(denied.status, 1);
  assert.equal(denied.stderr, "PermissionError\n");
  const narrow = spawnSync(
    process.execPath,
    [
      new URL("../../fixtures/narrow-access-probe.ts", import.meta.url).pathname,
      path,
      String(address.port),
      "NARROW_ACCESS_DENIED",
    ],
    { encoding: "utf8" },
  );
  assert.equal(narrow.status, 1);
  assert.equal(narrow.stdout, "");
  assert.match(narrow.stderr, /Sandbox unexpectedly allowed NARROW_ACCESS_DENIED/);
});
