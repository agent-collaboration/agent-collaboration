// The seed Worker can only be configured for a staging central.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { WorkerConfig } from "../../../tools/central-migration/scripts/deploy_configs.ts";
import { seedWorkerConfig } from "../scripts/deploy_config.ts";

const target = (name: string, domains: string[] = []): WorkerConfig => {
  const binding = (exportName: string) => ({ type: "durable-object", worker: name, exportName });
  return {
    worker: {
      name,
      entrypoint: "artifact/index.js",
      domains,
      env: {
        CONNECTIONS: binding("ConnectionObject"),
        FLEET: binding("FleetObject"),
        LEASE_REGISTRY: binding("LeaseRegistryObject"),
        MAILBOXES: binding("MailboxObject"),
        PROJECT_COPIES: binding("ProjectCopyObject"),
      },
      exports: Object.fromEntries(
        [
          "ConnectionObject",
          "FleetObject",
          "LeaseRegistryObject",
          "MailboxObject",
          "ProjectCopyObject",
        ].map((name) => [name, { type: "durable-object", storage: "sqlite" } as const]),
      ),
    },
  };
};

test("a staging target gets the seed entrypoint and its own name as the seed target", () => {
  const config = seedWorkerConfig(target("collaboration-central-staging"), "build/worker.js");
  assert.equal(config.worker.entrypoint, "build/worker.js");
  assert.deepEqual(config.worker.env.STAGING_SEED_TARGET, {
    type: "text",
    value: "collaboration-central-staging",
  });
});

test("production's central, or a target not named staging, is refused", () => {
  assert.throws(() => seedWorkerConfig(target("collaboration-central"), "w.js"), /production/);
  assert.throws(
    () =>
      seedWorkerConfig(
        target("collaboration-central-staging", ["central.agent-collaboration.com"]),
        "w.js",
      ),
    /production/,
  );
  assert.throws(
    () => seedWorkerConfig(target("collaboration-central-test"), "w.js"),
    /doesn't say staging/,
  );
});
