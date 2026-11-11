import assert from "node:assert/strict";
import { test } from "node:test";
import {
  requirePreservedRootFiles,
  requireCarriedDisabledProviders,
} from "./installation-root-update.ts";

test("a stopped updater may resume only with its generated disabled-provider settings", (): void => {
  const header =
    "// Router settings, carried unchanged from providers.json and ports.json by\n" +
    "// agent-router-update. Edit by hand; the Host reads it at each start.\n";
  const providers = {
    version: 1,
    providers: {
      claude: { enabled: false, executable: null, arguments: [] },
      cursor: { enabled: false, executable: null, arguments: [] },
    },
  };
  assert.doesNotThrow(() =>
    requireCarriedDisabledProviders(header + JSON.stringify({ providers })),
  );
  assert.throws(
    () => requireCarriedDisabledProviders(JSON.stringify({ providers })),
    /updater-generated/,
  );
  assert.throws(() =>
    requireCarriedDisabledProviders(
      header +
        JSON.stringify({
          providers: {
            ...providers,
            providers: { ...providers.providers, claude: { enabled: true } },
          },
        }),
    ),
  );
  assert.throws(() => requireCarriedDisabledProviders(header + JSON.stringify({ unrelated: {} })));
});

test("root update cannot lose or replace enrollment, identity or project bytes", (): void => {
  const before = { identity: "a".repeat(64), enrollment: "b".repeat(64), project: "c".repeat(64) };
  assert.doesNotThrow(() => requirePreservedRootFiles(before, { ...before }));
  assert.throws(
    () => requirePreservedRootFiles(before, { ...before, enrollment: "d".repeat(64) }),
    /enrollment or project files/,
  );
  assert.throws(
    () =>
      requirePreservedRootFiles(before, {
        identity: before.identity,
        enrollment: before.enrollment,
      }),
    /enrollment or project files/,
  );
});
