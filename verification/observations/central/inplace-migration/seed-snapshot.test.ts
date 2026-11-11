import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { encode } from "../../../exact-json.ts";
import { classifySeedHeader, validateSeedSnapshot } from "./seed-snapshot.ts";

const project = "01a1c0b1-0000-7000-8000-000000000001";
const holder = "01a1c0b1-0000-7000-8000-000000000002";
const receiver = "01a1c0b1-0000-7000-8000-000000000003";
const head = { epoch: 2n, sequence: 12n };
const schema = { version: 5n, fingerprint: "fixture-schema" };
const sourceHash = "a".repeat(64);
function fixtureHeader() {
  return {
    kind: "seedSnapshotHeader",
    database: project,
    holder,
    registration: { Ready: { lease: { database: project, holder, epoch: 3n } } },
    writerState: { state: "paused", epoch: 3n },
    position: head,
    schema,
    copies: [
      {
        copy: receiver,
        role: "authoritative",
        start: { epoch: 1n, sequence: 0n },
        acked: { epoch: 2n, sequence: 10n },
        needsSnapshot: false,
      },
    ],
  };
}
function records(): unknown[] {
  return [
    fixtureHeader(),
    {
      kind: "seedSnapshotPage",
      page: {
        database: project,
        position: head,
        schema,
        table: "facts",
        rows: [["retained"]],
        schemaSteps: ["CREATE TABLE facts(id TEXT PRIMARY KEY)"],
        next: null,
      },
    },
    {
      kind: "seedSnapshotManifest",
      manifest: {
        database: project,
        position: head,
        schema,
        tables: [{ table: "facts", rowCount: 1n, digest: "SDK-owned-digest" }],
      },
    },
    {
      kind: "seedSnapshotComplete",
      database: project,
      holder,
      position: head,
      untouchedBeforeSha256: sourceHash,
      untouchedAfterSha256: sourceHash,
      retainedFloor: null,
      retainedFloorObserved: false,
      currentRenewalObserved: false,
    },
  ];
}
async function validate(values: unknown[]) {
  const directory = await mkdtemp(join(tmpdir(), "migration-snapshot-contract-"));
  const path = join(directory, "snapshot.jsonl");
  await writeFile(path, values.map(encode).join("\n") + "\n", { mode: 0o600, flag: "wx" });
  return validateSeedSnapshot({
    path,
    expectedProject: project,
    expectedHolder: holder,
    expectedLease: 3n,
    untouchedSha256: sourceHash,
  });
}
test("healthy tail keeps fence above position epoch and never invents a numeric floor", () => {
  const observed = classifySeedHeader(fixtureHeader(), { project, holder, lease: 3n });
  assert.equal(observed.hasTail, true);
  assert.equal(observed.classification, "readyForBaseline");
  assert.equal(observed.observed.registration.Ready.lease.epoch, 3n);
  assert.equal(observed.retainedFloorObserved, false);
  const unhealthy = fixtureHeader();
  const [copy] = unhealthy.copies;
  assert.ok(copy);
  copy.needsSnapshot = true;
  assert.equal(
    classifySeedHeader(unhealthy, { project, holder, lease: 3n }).classification,
    "needsSnapshot",
  );
});
test("wrong writer, duplicate receivers and ACK beyond head cannot qualify", () => {
  assert.throws(
    () => classifySeedHeader(fixtureHeader(), { project, holder, lease: 4n }),
    /fence/u,
  );
  assert.throws(
    () => classifySeedHeader(fixtureHeader(), { project, holder: receiver, lease: 3n }),
    /identity/u,
  );
  const duplicated = fixtureHeader();
  duplicated.copies.push(...duplicated.copies);
  assert.throws(() => classifySeedHeader(duplicated, { project, holder, lease: 3n }), /duplicate/u);
  const ahead = fixtureHeader();
  const [copy] = ahead.copies;
  assert.ok(copy);
  copy.acked = { epoch: 2n, sequence: 13n };
  assert.throws(() => classifySeedHeader(ahead, { project, holder, lease: 3n }), /outside/u);
});
test("a truncated export or duplicate body cannot be treated as a complete seed", async () => {
  const valid = await validate(records());
  assert.equal(valid.rows, 1n);
  assert.equal(valid.tables, 1);
  await assert.rejects(validate(records().slice(0, -1)), /incomplete/u);
  const duplicated = records();
  duplicated.splice(2, 0, duplicated[1]);
  await assert.rejects(validate(duplicated), /row counts/u);
  const trailing = records();
  trailing.push(fixtureHeader());
  await assert.rejects(validate(trailing), /framing/u);
});
