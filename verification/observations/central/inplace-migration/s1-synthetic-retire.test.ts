import assert from "node:assert/strict";
import { test } from "node:test";
import { directoryEntry } from "./contracts.ts";
import {
  dedicatedRetirementProject,
  retireOnce,
  validateRetirementFacts,
} from "./s1-synthetic-retire.ts";

const board = "01a1c438-6b0d-7802-926f-ef51b2a6eef4";
const peer = "01a175ad-b72a-7c33-aafb-53b08e82b03f";
const fixture = {
  projectReadback: { record: { project: { id: dedicatedRetirementProject, board } } },
  board: { record: { board: { boardId: board, state: "archived" } } },
  directory: {
    project: dedicatedRetirementProject,
    name: "synthetic",
    workspace: "01a1c438-6b0d-75bf-a2b7-bd5758d1edd8",
    board,
    boardArchive: "archived",
    copySide: { kind: "notYetReported" },
    leaseSide: {
      kind: "known",
      value: {
        lease: { kind: "unheld" },
        leaseVersion: { epoch: 1n, incarnation: 1n, change: 1n },
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
          holder: null,
          start: { epoch: 1n, sequence: 0n },
          acked: { epoch: 1n, sequence: 57n },
          schema: { version: 1n, fingerprint: "schema" },
        },
      },
      projectFingerprint: { kind: "known", value: "schema" },
      stagedParts: [],
      storageBytes: 100n,
    },
  },
};
const retiredDirectory = {
  ...fixture.directory,
  leaseSide: {
    kind: "known",
    value: {
      ...fixture.directory.leaseSide.value,
      lease: { kind: "retired", value: { retired_at: "2026-11-10T02:00:00Z" } },
    },
  },
};
const before = (): Parameters<typeof retireOnce>[1] => ({
  cohort: [directoryEntry.parse(fixture.directory)],
  facts: validateRetirementFacts(fixture),
});

test("normal retirement accepts an archived unheld project without a holder preview", (): void => {
  assert.deepEqual(validateRetirementFacts(fixture), {
    fence: 1n,
    acked: { epoch: 1n, sequence: 57n },
  });
});
test("retirement rejects a held grant, another project, active board or non-authoritative holder", (): void => {
  const changes: Parameters<typeof validateRetirementFacts>[0][] = [
    { ...fixture, projectReadback: { record: { project: { id: peer, board } } } },
    { ...fixture, board: { record: { board: { boardId: board, state: "archiving" } } } },
    { ...fixture, directory: { ...fixture.directory, boardArchive: "open" } },
    {
      ...fixture,
      directory: {
        ...fixture.directory,
        leaseSide: {
          kind: "known",
          value: {
            ...fixture.directory.leaseSide.value,
            lease: {
              kind: "held",
              value: {
                epoch: 1n,
                holder: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89",
                state: "held",
                expires_at: "2026-11-10T06:00:00Z",
              },
            },
          },
        },
      },
    },
    {
      ...fixture,
      health: {
        ...fixture.health,
        value: {
          ...fixture.health.value,
          copy: { kind: "known", value: { ...fixture.health.value.copy.value, holder: peer } },
        },
      },
    },
    {
      ...fixture,
      health: {
        ...fixture.health,
        value: {
          ...fixture.health.value,
          copy: { kind: "known", value: { ...fixture.health.value.copy.value, role: "mirror" } },
        },
      },
    },
    {
      ...fixture,
      health: {
        ...fixture.health,
        value: {
          ...fixture.health.value,
          copy: { kind: "known", value: { ...fixture.health.value.copy.value, fence: 0n } },
        },
      },
    },
    {
      ...fixture,
      health: {
        ...fixture.health,
        value: {
          ...fixture.health.value,
          copy: { kind: "known", value: { ...fixture.health.value.copy.value, database: peer } },
        },
      },
    },
  ];
  for (const changed of changes) assert.throws(() => validateRetirementFacts(changed));
});

for (const failure of [
  {
    exit: 4,
    answer: { kind: "refused", value: { kind: "leaseHeld" } },
    expected: /explicitly refused/,
  },
  { exit: 3, answer: { kind: "objectUnavailable" }, expected: /unavailable/ },
]) {
  test(`known CLI ${failure.answer.kind} does not poll or resend`, async (): Promise<void> => {
    const calls: string[] = [];
    const saved = new Map<string, unknown>();
    const cli = {
      async save(name: string, value: unknown): Promise<void> {
        saved.set(name, value);
      },
      async execute(name: string): Promise<unknown> {
        calls.push(name);
        if (name !== "retire-once") throw new Error("unexpected read-only reconciliation");
        throw new Error("native CLI answer", {
          cause: { kind: "syntheticCliAnswer", exit: failure.exit, answer: failure.answer },
        });
      },
    };
    await assert.rejects(retireOnce(cli, before()), failure.expected);
    assert.deepEqual(calls, ["retire-once"]);
    assert.equal(saved.has("retirement-outcome-unknown"), false);
  });
}

function completedPublicReads(arguments_: readonly string[]): unknown {
  if (arguments_[1] === "copy")
    return { kind: "retired", value: { retiredAt: "2026-11-10T02:00:00Z" } };
  if (arguments_[1] === "copies")
    return arguments_.includes("--after")
      ? { projects: [], nextAfter: null, asOf: "2026-11-10T02:00:00Z" }
      : {
          projects: [retiredDirectory],
          nextAfter: dedicatedRetirementProject,
          asOf: "2026-11-10T02:00:00Z",
        };
  throw new Error("unexpected public read");
}

test("known Pending retrieves terminal owner result with the identical request and fresh fence/ACK", async (): Promise<void> => {
  const calls: string[][] = [];
  const cli = {
    async save(_name: string, _value: unknown): Promise<void> {},
    async execute(name: string, arguments_: readonly string[]): Promise<unknown> {
      if (name === "retire-once" || name === "retire-terminal-same-request") {
        calls.push([...arguments_]);
        const requestId = arguments_[arguments_.indexOf("--request-id") + 1];
        if (requestId === undefined) throw new Error("missing saved identity");
        return name === "retire-once"
          ? { kind: "pending", value: { requestId } }
          : { kind: "retired", value: { at: 1794276000000n } };
      }
      return completedPublicReads(arguments_);
    },
  };
  const result = await retireOnce(cli, before());
  assert.equal(result.actualOwnerRetiredReply, true);
  assert.equal(result.ownerInvocations, 2);
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[0], calls[1]);
  assert.deepEqual(calls[0]?.slice(-4), [
    "--expect-fence",
    "1",
    "--expect-acked",
    '{"epoch":1,"sequence":57}',
  ]);
});

test("public Retired plus another Pending owner answer is still incomplete", async (): Promise<void> => {
  let ownerInvocations = 0;
  const cli = {
    async save(_name: string, _value: unknown): Promise<void> {},
    async execute(name: string, arguments_: readonly string[]): Promise<unknown> {
      if (name === "retire-once" || name === "retire-terminal-same-request") {
        ownerInvocations += 1;
        return {
          kind: "pending",
          value: { requestId: arguments_[arguments_.indexOf("--request-id") + 1] },
        };
      }
      return completedPublicReads(arguments_);
    },
  };
  const result = await retireOnce(cli, before());
  assert.equal(result.actualOwnerRetiredReply, false);
  assert.equal(result.ownerInvocations, 2);
  assert.equal(ownerInvocations, 2);
});

test("an unconfirmed answer reconciles only by reads and never claims an owner result", async (): Promise<void> => {
  let ownerInvocations = 0;
  const saved = new Map<string, unknown>();
  const cli = {
    async save(name: string, value: unknown): Promise<void> {
      saved.set(name, value);
    },
    async execute(name: string, arguments_: readonly string[]): Promise<unknown> {
      if (name === "retire-once" || name === "retire-terminal-same-request") {
        ownerInvocations += 1;
        throw new Error("transport answer lost");
      }
      return completedPublicReads(arguments_);
    },
  };
  await assert.rejects(retireOnce(cli, before()), /answer unknown/);
  assert.equal(ownerInvocations, 1);
  assert.equal(saved.has("retirement-outcome-unknown"), true);
  assert.equal(saved.has("retirement-public-state-fact"), true);
});
