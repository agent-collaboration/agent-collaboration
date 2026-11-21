import assert from "node:assert/strict";
import { test } from "node:test";
import { adaptRuntimeLaunch } from "./owned-auto-runtime.ts";
import type { RuntimeLaunch } from "../../runtime/runtime-plan.ts";

const plan = { router_root: "/owned/debug-root", claude_adapter: "/qualified/adapter/index.js" };
const input: RuntimeLaunch = {
  command: [
    "/signed/router",
    "serve",
    "--state-db",
    "/owned/debug-root/provider-proxy.sqlite",
    "--port",
    "28871",
  ],
  stdout: "/evidence/router.stdout",
  stderr: "/evidence/router.stderr",
};

test("current-layout proxy launch preserves the canonical database argument", (): void => {
  const adapted = adaptRuntimeLaunch({ input, plan, socketDirectory: "/private/socket" });
  assert.deepEqual(adapted.command, [
    "/signed/router",
    "serve",
    "--state-db",
    "/owned/debug-root/provider-proxy.sqlite",
    "--port",
    "28871",
  ]);
  assert.equal(input.command[3], "/owned/debug-root/provider-proxy.sqlite");
  assert.equal(adapted.stdout, input.stdout);
  assert.equal(adapted.stderr, input.stderr);
  assert.throws(
    (): unknown =>
      adaptRuntimeLaunch({
        input: {
          ...input,
          command: ["/signed/router", "serve", "--state-db", "/other/account.sqlite"],
        },
        plan,
        socketDirectory: "/private/socket",
      }),
    /proxy command changed/,
  );
});

test("Host adaptation uses qualified stock Claude and short private socket without changing saved files", (): void => {
  const host = {
    ...input,
    command: ["/signed/router", "host", "--claude-acp-executable", plan.claude_adapter],
    environment: {
      CODEX_HOME: "/normal/codex",
      CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET: "/too/long/socket",
      KEEP: "value",
    },
  };
  const adapted = adaptRuntimeLaunch({ input: host, plan, socketDirectory: "/private/socket" });
  assert.deepEqual(adapted.command, [
    "/signed/router",
    "host",
    "--claude-acp-executable",
    plan.claude_adapter,
  ]);
  assert.equal(
    adapted.environment?.["CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET"],
    "/private/socket/app.sock",
  );
  assert.equal(adapted.environment?.["CODEX_HOME"], "/normal/codex");
  assert.equal(adapted.environment?.["KEEP"], "value");
  assert.equal(host.environment.CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET, "/too/long/socket");
  assert.throws(
    (): unknown =>
      adaptRuntimeLaunch({
        input: { ...host, command: ["/signed/router", "host"] },
        plan,
        socketDirectory: "/private/socket",
      }),
    /Claude launch changed/,
  );
});
