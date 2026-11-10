import assert from "node:assert/strict";
import { test } from "node:test";
import { dedicatedRetirementProject, validateRetirementFacts } from "./s1-synthetic-retire.ts";

const board = "01a1c307-6ffd-716c-9957-0b382a7018e4";
const installation = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
const peer = "01a175ad-b72a-7c33-aafb-53b08e82b03f";
const fixture = {
  installation,
  peer,
  local: { record: { project: { id: dedicatedRetirementProject, board } } },
  board: { record: { board: { boardId: board, state: "archived" } } },
  preview: {
    preview: {
      token: "synthetic-private-preview-token",
      holder: peer,
      fence: 1n,
      acked: { epoch: 1n, sequence: 65n },
    },
    holderInstallation: installation,
  },
  directory: {
    project: dedicatedRetirementProject,
    name: "synthetic",
    workspace: "01a1c2fb-e1ec-735b-aea3-70d8f48b01b9",
    board,
    boardArchive: "archived",
    copySide: { kind: "notYetReported" },
    leaseSide: {
      kind: "known",
      value: {
        lease: {
          kind: "held",
          value: {
            epoch: 1n,
            holder: installation,
            state: "held",
            expires_at: "2026-11-10T06:00:00Z",
          },
        },
        leaseVersion: { epoch: 1n, incarnation: 1n, change: 3n },
        asOf: "2026-11-10T02:00:00Z",
      },
    },
  },
  health: {
    kind: "live",
    value: {
      bookkeeping: [],
      migrationFailures: [],
      stage: { kind: "known", value: "following" },
      copy: {
        kind: "known",
        value: {
          database: dedicatedRetirementProject,
          role: "authoritative",
          fence: 1n,
          holder: peer,
          start: { epoch: 1n, sequence: 0n },
          acked: { epoch: 1n, sequence: 65n },
          schema: { version: 1n, fingerprint: "schema" },
        },
      },
      projectFingerprint: { kind: "known", value: "schema" },
      stagedParts: [],
      storageBytes: 100n,
    },
  },
};
test("retirement uses the fresh dedicated archived project's matching installation/peer/fence/ACK", (): void => {
  assert.deepEqual(validateRetirementFacts(fixture).preview.acked, { epoch: 1n, sequence: 65n });
});
test("retirement refuses another project, non-archived board or stale/foreign preview before sending", (): void => {
  const changes: ((value: typeof fixture) => void)[] = [
    (value): void => {
      value.local.record.project.id = "01a1c2fb-e1ec-76c3-9597-0836e606c205";
    },
    (value): void => {
      value.board.record.board.state = "archiving";
    },
    (value): void => {
      value.preview.holderInstallation = peer;
    },
    (value): void => {
      value.preview.preview.holder = dedicatedRetirementProject;
    },
    (value): void => {
      value.preview.preview.fence = 2n;
    },
    (value): void => {
      value.preview.preview.acked.sequence = 64n;
    },
    (value): void => {
      value.health.value.copy.value.holder = dedicatedRetirementProject;
    },
    (value): void => {
      value.directory.boardArchive = "open";
    },
  ];
  for (const change of changes) {
    const changed = structuredClone(fixture);
    change(changed);
    assert.throws(() => validateRetirementFacts(changed));
  }
});
