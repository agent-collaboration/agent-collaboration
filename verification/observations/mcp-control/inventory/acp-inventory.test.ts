import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  extractLockedNames,
  extractSessionUpdates,
  inventoryDefects,
  loadAcpInventory,
  lockedCoverageDefects,
  lockedSchemaVersion,
  toolCoverageDefects,
  type AcpInventory,
  type LockedName,
} from "./acp-inventory.ts";

const repository = join(dirname(fileURLToPath(import.meta.url)), "../../../..");
const inventory = loadAcpInventory();
const lockedFromInventory = (rows: AcpInventory["rows"]): LockedName[] =>
  rows.flatMap((row): LockedName[] =>
    row.kind === "extension" ? [] : [{ name: row.name, kind: row.kind, gate: row.gate }],
  );

test("the inventory is keyed to the schema version this workspace locks", () => {
  const cargoLock = readFileSync(join(repository, "Cargo.lock"), "utf8");
  assert.equal(lockedSchemaVersion(cargoLock, inventory.schemaCrate), inventory.schemaVersion);
  assert.deepEqual(inventoryDefects(inventory), []);
});

test("an added, removed or re-gated locked member fails coverage", () => {
  const locked = lockedFromInventory(inventory.rows);
  assert.deepEqual(lockedCoverageDefects(inventory, locked), []);
  const added = [...locked, { name: "session/archive", kind: "method" as const, gate: null }];
  assert.deepEqual(lockedCoverageDefects(inventory, added), [
    "locked method session/archive has no inventory row",
  ]);
  const removed = locked.filter((member) => member.name !== "session/delete");
  assert.deepEqual(lockedCoverageDefects(inventory, removed), [
    "method session/delete is not in the locked schema",
  ]);
  const regated = locked.map((member) =>
    member.name === "session/resume" ? { ...member, gate: "unstable_x" } : member,
  );
  assert.equal(lockedCoverageDefects(inventory, regated).length, 1);
});

test("a guessed positive, a proof without a claim and an unoffered tool are defects", () => {
  const guessed: AcpInventory = {
    ...inventory,
    rows: inventory.rows.map((row) =>
      row.name === "session/delete" ? { ...row, disposition: "qualified" } : row,
    ),
  };
  assert.deepEqual(inventoryDefects(guessed), [
    "session/delete claims qualification without a tool and a deciding case",
  ]);
  const ungated: AcpInventory = {
    ...inventory,
    rows: inventory.rows.map((row) =>
      row.name === "session/fork" ? { ...row, disposition: "represented" } : row,
    ),
  };
  assert.deepEqual(inventoryDefects(ungated), [
    "session/fork: compile gate and disposition disagree",
  ]);
  const offered = inventory.rows
    .flatMap((row) => row.tools)
    .filter((tool) => tool !== "conversation_resume");
  assert.deepEqual(toolCoverageDefects(inventory, offered), [
    "session/resume names conversation_resume, which tools/list does not offer",
  ]);
});

test("extraction reads constants, attribute and module gates, and SessionUpdate variants", () => {
  const directory = mkdtempSync(join(tmpdir(), "acp-schema-"));
  try {
    writeFileSync(
      join(directory, "agent.rs"),
      [
        'pub(crate) const SESSION_NEW_METHOD_NAME: &str = "session/new";',
        'pub(crate) const SESSION_CANCEL_METHOD_NAME: &str = "session/cancel";',
        "/// Forks a session.",
        '#[cfg(feature = "unstable_session_fork")]',
        'pub(crate) const SESSION_FORK_METHOD_NAME: &str = "session/fork";',
        'pub(crate) const LOGOUT_METHOD_NAME: &str = "logout";',
      ].join("\n"),
    );
    writeFileSync(
      join(directory, "nes.rs"),
      'pub(crate) const NES_START_METHOD_NAME: &str = "nes/start";\n',
    );
    writeFileSync(
      join(directory, "client.rs"),
      [
        'pub(crate) const SESSION_UPDATE_NOTIFICATION: &str = "session/update";',
        "pub enum SessionUpdate {",
        "    AgentMessageChunk(ContentChunk),",
        '    #[cfg(feature = "unstable_session_notices")]',
        "    Notice(Notice),",
        "    UsageUpdate(UsageUpdate),",
        "}",
      ].join("\n"),
    );
    assert.deepEqual(
      extractLockedNames(directory).sort((left, right) => left.name.localeCompare(right.name)),
      [
        { name: "logout", kind: "method", gate: null },
        { name: "nes/start", kind: "method", gate: "unstable_nes" },
        { name: "session/cancel", kind: "notification", gate: null },
        { name: "session/fork", kind: "method", gate: "unstable_session_fork" },
        { name: "session/new", kind: "method", gate: null },
        { name: "session/update", kind: "notification", gate: null },
      ],
    );
    assert.deepEqual(extractSessionUpdates(readFileSync(join(directory, "client.rs"), "utf8")), [
      { name: "agent_message_chunk", kind: "sessionUpdate", gate: null },
      { name: "notice", kind: "sessionUpdate", gate: "unstable_session_notices" },
      { name: "usage_update", kind: "sessionUpdate", gate: null },
    ]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
