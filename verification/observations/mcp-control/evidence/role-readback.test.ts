import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { ProviderStoreReadback, storedIdentityIs } from "./role-readback.ts";

const TARGET = { endpoint: { serviceId: "svc", endpointId: "cursor-local" }, sessionId: "s-1" };
const AGENT = { endpoint: { serviceId: "svc", endpointId: "codex-local" }, sessionId: "agent-1" };

// The two owning tables with the columns this readback selects (collaboration-runtime
// migrations 202609200001 and 202609240002); the store under test is a disposable copy.
function seededStore(directory: string): string {
  const path = join(directory, "provider-operations.sqlite");
  const database = new DatabaseSync(path);
  database.exec(`
    CREATE TABLE provider_session_records (target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, created_by_json TEXT, approver_json TEXT);
    CREATE TABLE provider_operations (operation_id TEXT, operation_kind TEXT, target_service_id TEXT, target_endpoint_id TEXT, target_session_id TEXT, stage TEXT, effect TEXT, reconciliation_state TEXT, admitted_at_ms INTEGER, terminal_at_ms INTEGER);
  `);
  database
    .prepare("INSERT INTO provider_session_records VALUES (?, ?, ?, ?, ?)")
    .run("svc", "cursor-local", "s-1", JSON.stringify({ humanId: "owner" }), JSON.stringify(AGENT));
  const operation = database.prepare(
    "INSERT INTO provider_operations VALUES (?, ?, 'svc', ?, ?, ?, ?, ?, ?, ?)",
  );
  operation.run(
    "op-1",
    "conversationCreate",
    "cursor-local",
    "s-1",
    "terminal",
    "applied",
    "confirmed",
    1,
    2,
  );
  operation.run(
    "op-2",
    "conversationPrompt",
    "cursor-local",
    "s-1",
    "dispatched",
    "unknown",
    "pending",
    3,
    null,
  );
  operation.run(
    "op-3",
    "conversationPrompt",
    "claude-local",
    "s-1",
    "terminal",
    "applied",
    "confirmed",
    4,
    5,
  );
  database.close();
  return path;
}

test("stored Human and Session roles decode exactly and are never confused", () => {
  const directory = mkdtempSync(join(tmpdir(), "role-readback-"));
  try {
    const store = new ProviderStoreReadback(seededStore(directory));
    const roles = store.sessionRoles(TARGET);
    assert.deepEqual(roles, {
      createdBy: { kind: "human", humanId: "owner" },
      approver: { kind: "session", session: AGENT },
    });
    assert.ok(
      roles !== null && storedIdentityIs(roles.createdBy, { kind: "human", humanId: "owner" }),
    );
    assert.ok(
      roles !== null && !storedIdentityIs(roles.approver, { kind: "human", humanId: "owner" }),
    );
    assert.equal(store.sessionRoles({ ...TARGET, sessionId: "missing" }), null);
    store.close();
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("operations are read per exact target and unsettled work stays visible", () => {
  const directory = mkdtempSync(join(tmpdir(), "role-readback-"));
  try {
    const store = new ProviderStoreReadback(seededStore(directory));
    assert.deepEqual(
      store.operations(TARGET).map((row) => [row.operationId, row.terminal]),
      [
        ["op-1", true],
        ["op-2", false],
      ],
    );
    assert.equal(store.nonTerminalCount(), 1);
    store.close();
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
