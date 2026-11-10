import assert from "node:assert/strict";
import { test } from "node:test";
import { healthReply } from "./contracts.ts";
import { directoryEntry } from "./contracts.ts";
import { collectSelectedRegistry, matchRegistryCopy } from "./s1-registry-readback.ts";

const project = "01900000-0000-7000-8000-000000000001";
const other = "01900000-0000-7000-8000-000000000002";
const holder = "01900000-0000-7000-8000-000000000003";
const installation = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
const expected = {
  expectation: "held",
  project,
  holderInstallation: installation,
  holderPeer: holder,
  lease: 2n,
} as const;
function entry(id = project): ReturnType<typeof directoryEntry.parse> {
  return directoryEntry.parse({
    project: id,
    name: "literal",
    workspace: other,
    board: other,
    boardArchive: "archived",
    copySide: { kind: "notYetReported" },
    leaseSide: {
      kind: "known",
      value: {
        lease: {
          kind: "held",
          value: {
            epoch: 2n,
            holder: installation,
            state: "held",
            expires_at: "2026-11-10T06:00:00Z",
          },
        },
        leaseVersion: { epoch: 2n, incarnation: 3n, change: 8n },
        asOf: "2026-11-10T02:00:00Z",
      },
    },
  });
}
function health(): ReturnType<typeof healthReply.parse> {
  return healthReply.parse({
    kind: "live",
    value: {
      bookkeeping: [],
      migrationFailures: [],
      stage: { kind: "known", value: "following" },
      copy: {
        kind: "known",
        value: {
          database: project,
          role: "authoritative",
          fence: 2n,
          holder,
          start: { epoch: 2n, sequence: 0n },
          acked: { epoch: 2n, sequence: 7n },
          schema: { version: 1n, fingerprint: "schema" },
        },
      },
      projectFingerprint: { kind: "known", value: "schema" },
      stagedParts: [],
      storageBytes: 100n,
    },
  });
}
test("an archived board can retain its held Registry grant; wrong D/M/N and missing Copy fail", (): void => {
  matchRegistryCopy({ expected, entry: entry(), health: health() });
  const wrongCopy = health();
  if (
    wrongCopy.kind !== "live" ||
    wrongCopy.value.copy.kind !== "known" ||
    wrongCopy.value.copy.value === null
  )
    throw new Error("live fixture required");
  wrongCopy.value.copy.value.holder = other;
  assert.throws(
    () => matchRegistryCopy({ expected, entry: entry(), health: wrongCopy }),
    /holder or fence differs/,
  );
  assert.throws(
    () =>
      matchRegistryCopy({
        expected: { ...expected, holderInstallation: other },
        entry: entry(),
        health: health(),
      }),
    /D\/M\/N/,
  );
  assert.throws(
    () =>
      matchRegistryCopy({ expected: { ...expected, lease: 3n }, entry: entry(), health: health() }),
    /D\/M\/N/,
  );
  assert.throws(
    () => matchRegistryCopy({ expected, entry: entry(other), health: health() }),
    /authoritative Registry/,
  );
  assert.throws(
    () =>
      matchRegistryCopy({
        expected,
        entry: entry(),
        health: { kind: "retired", value: { retiredAt: "now" } },
      }),
    /Copy facts/,
  );
});
test("complete Registry paging precedes selected Copy reads and missing membership never passes", async (): Promise<void> => {
  const requests: string[] = [];
  const read = async (
    path: string,
  ): Promise<{ body: unknown; artifact: string; sha256: string }> => {
    requests.push(path);
    const body = path.endsWith("/health")
      ? health()
      : path.includes("&after=")
        ? { projects: [], nextAfter: null, asOf: "now" }
        : { projects: [entry()], nextAfter: project, asOf: "now" };
    return { body, artifact: "private-receipt", sha256: "a".repeat(64) };
  };
  const observed = await collectSelectedRegistry({ expected: [expected], read });
  assert.equal(observed.projects.length, 1);
  assert.deepEqual(requests, [
    "/admin/v1/copies?limit=100",
    `/admin/v1/copies?limit=100&after=${project}`,
    `/admin/v1/copies/${project}/health`,
  ]);
  await assert.rejects(
    collectSelectedRegistry({ expected: [{ ...expected, project: other }], read }),
    /missing from complete Registry/,
  );
});
