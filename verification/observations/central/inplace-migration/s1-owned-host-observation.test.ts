import assert from "node:assert/strict";
import { test } from "node:test";
import type { CommandLedgerEntry } from "../../../machines/command-ledger.ts";
import { stoppedOwnedRootSchema } from "../../../machines/installation-stopped-root.ts";
import { requireSelectedOwnedHost, requireOwnedWindowLink } from "./s1-owned-host-observation.ts";
const root = "/private/tmp/2026-11-s1-fixture/router",
  executable = "/private/tmp/2026-11-s1-fixture/old125";
const oldId = "01900000-0000-7000-8000-000000000001",
  currentId = "01900000-0000-7000-8000-000000000002";
const openedAt = "2026-11-10T05:00:00.000Z",
  at = "2026-11-10T05:00:01.000Z";
const current = { pid: 900002, commandId: currentId };
function ledger(): CommandLedgerEntry[] {
  return [
    {
      kind: "started",
      at: "2026-11-09T00:00:00.000Z",
      commandId: oldId,
      mode: "daemon",
      executable,
      arguments: ["host", "--router-root", root, "--port", "18827"],
      cwd: root,
      environmentKeys: [],
    },
    { kind: "spawned", at, commandId: oldId, pid: 900001 },
    { kind: "ended", at, commandId: oldId, pid: 900001, exit: 0, signal: null, error: null },
    {
      kind: "started",
      at,
      commandId: currentId,
      mode: "daemon",
      executable,
      arguments: ["host", "--router-root", root, "--port", "18827"],
      cwd: root,
      environmentKeys: [],
    },
    { kind: "spawned", at, commandId: currentId, pid: 900002 },
  ];
}
test("current generation is explicitly selected among multiple historical Host starts", () => {
  const rows = ledger();
  assert.deepEqual(
    requireSelectedOwnedHost({
      host: current,
      root,
      executable,
      port: 18827,
      openedAt,
      ledger: rows,
    }),
    { ...current, startedAt: at },
  );
  for (const host of [
    { pid: 900001, commandId: oldId },
    { pid: 900001, commandId: currentId },
  ])
    assert.throws(
      () =>
        requireSelectedOwnedHost({ host, root, executable, port: 18827, openedAt, ledger: rows }),
      /still-running/,
    );
});
test("ended, missing, foreign-root, replaced-artifact and earlier-window selections refuse", () => {
  const input = { host: current, root, executable, port: 18827, openedAt, ledger: ledger() };
  assert.throws(
    () =>
      requireSelectedOwnedHost({
        ...input,
        ledger: [
          ...input.ledger,
          {
            kind: "ended",
            at,
            commandId: currentId,
            pid: 900002,
            exit: 0,
            signal: null,
            error: null,
          },
        ],
      }),
    /still-running/,
  );
  assert.throws(
    () =>
      requireSelectedOwnedHost({
        ...input,
        ledger: input.ledger.filter((row) => row.kind !== "spawned"),
      }),
    /still-running/,
  );
  assert.throws(
    () => requireSelectedOwnedHost({ ...input, root: `${root}-other` }),
    /owned generation/,
  );
  assert.throws(
    () => requireSelectedOwnedHost({ ...input, executable: `${executable}-other` }),
    /owned generation/,
  );
  assert.throws(
    () => requireSelectedOwnedHost({ ...input, openedAt: "2026-11-10T06:00:00.000Z" }),
    /predates/,
  );
  assert.throws(
    () =>
      requireSelectedOwnedHost({
        ...input,
        ledger: [...input.ledger, input.ledger[3]].filter((row) => row !== undefined),
      }),
    /owned generation/,
  );
});
test("window binding keeps original baseline/window/controller references exact", () => {
  const ref = { path: "/private/tmp/2026-11-s1-fixture/original.json", sha256: "a".repeat(64) };
  const declaration = stoppedOwnedRootSchema.parse({
    kind: "stoppedOwnedRootContinuation",
    root,
    creatorUid: 501,
    evidenceDirectory: "/private/tmp/2026-11-s1-fixture/continuation",
    original: { plan: ref, baseline: ref, hostWindow: ref, observations: ref, cohort: ref },
    processes: { host: current, router: current },
    execution: {
      sourceHead: "cbf56bad640ae981900c646ff2ae0b64aaaebd3b",
      productVersion: "0.1.125",
      router: { ...ref, identifier: "router", team: "fixture" },
      cli: { ...ref, identifier: "cli", team: "fixture" },
    },
    controller: { sourceHead: "b".repeat(40), files: [ref] },
    filesCapturedAt: at,
    files: [{ project: oldId, sourceRelative: `${oldId}/e1.sqlite`, device: "1", inode: "2" }],
    homes: {
      home: "/fixture/home",
      codexHome: "/fixture/codex",
      nativeCwd: "/fixture",
      privateHomeTree: [],
      codexHomeTree: [],
    },
  });
  const window = {
    openedAt,
    originalWindow: ref,
    originalBaseline: ref,
    controller: declaration.controller,
  };
  const reference = {
    path: `${declaration.evidenceDirectory}/continuation-host-window.json`,
    sha256: "c".repeat(64),
  };
  requireOwnedWindowLink({ window, reference, declaration });
  assert.throws(
    () =>
      requireOwnedWindowLink({
        window: { ...window, originalBaseline: { ...ref, sha256: "d".repeat(64) } },
        reference,
        declaration,
      }),
    /original baseline/,
  );
  assert.throws(
    () =>
      requireOwnedWindowLink({ window, reference: { ...reference, path: ref.path }, declaration }),
    /original baseline/,
  );
  assert.throws(
    () =>
      requireOwnedWindowLink({
        window: { ...window, controller: { ...window.controller, sourceHead: "f".repeat(40) } },
        reference,
        declaration,
      }),
    /original baseline/,
  );
});
