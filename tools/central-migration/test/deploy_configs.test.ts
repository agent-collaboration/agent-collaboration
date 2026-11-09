// PD7's two deploy configurations, derived from a production-shaped target.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  mainCentralConfig,
  migrationWorkerConfig,
  type WorkerConfig,
} from "../scripts/deploy_configs.ts";

const binding = (exportName: string) => ({
  type: "durable-object",
  worker: "collaboration-central",
  exportName,
});

/** production's live-target.json `.config`, with placeholder values. */
const LIVE: WorkerConfig = {
  accountId: "account",
  worker: {
    name: "collaboration-central",
    entrypoint: "artifact/index.js",
    compatibilityDate: "2026-08-21",
    domains: ["central.example.test"],
    env: {
      ACCESS_ADMIN_AUD: { type: "text", value: "admin-aud" },
      CENTRAL_COPY_ACK_WAIT: { type: "text", value: '{"secs":20,"nanos":0}' },
      CONNECTIONS: binding("ConnectionObject"),
      FLEET: binding("FleetObject"),
      LEASE_REGISTRY: binding("LeaseRegistryObject"),
      MAILBOXES: binding("MailboxObject"),
      PROJECT_COPIES: binding("ProjectCopyObject"),
    },
    exports: {
      ConnectionObject: { type: "durable-object", storage: "sqlite" },
      ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      LeaseRegistryObject: { type: "durable-object", storage: "sqlite" },
      FleetObject: { type: "durable-object", storage: "sqlite" },
      MailboxObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};

test("the migration Worker deploys with the target's name, bindings and five classes", () => {
  // Act
  const config = migrationWorkerConfig(LIVE, "build/worker.js");

  // Assert: only the entrypoint differs.
  assert.deepEqual(config, { ...LIVE, worker: { ...LIVE.worker, entrypoint: "build/worker.js" } });
});

test("main's central keeps Fleet and ProjectCopy and deletes the three retired classes", () => {
  // Act
  const config = mainCentralConfig(LIVE, "build/index.js");

  // Assert
  assert.deepEqual(config.worker.exports, {
    FleetObject: { type: "durable-object", storage: "sqlite" },
    ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
    LeaseRegistryObject: { type: "durable-object", state: "deleted" },
    ConnectionObject: { type: "durable-object", state: "deleted" },
    MailboxObject: { type: "durable-object", state: "deleted" },
  });
  assert.deepEqual(Object.keys(config.worker.env).sort(), [
    "ACCESS_ADMIN_AUD",
    "CENTRAL_COPY_ACK_WAIT",
    "FLEET",
    "PROJECT_COPIES",
  ]);
  assert.equal(config.worker.name, "collaboration-central");
  assert.deepEqual(config.worker.domains, LIVE.worker.domains);
});

test("a target that isn't production's five-class central is refused", () => {
  const twoClasses: WorkerConfig = {
    ...LIVE,
    worker: {
      ...LIVE.worker,
      exports: {
        FleetObject: { type: "durable-object", storage: "sqlite" },
        ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      },
    },
  };
  assert.throws(
    () => mainCentralConfig(twoClasses, "build/index.js"),
    /expected the five central classes/,
  );
  assert.throws(
    () => migrationWorkerConfig(twoClasses, "build/worker.js"),
    /expected the five central classes/,
  );
});
