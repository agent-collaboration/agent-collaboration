import assert from "node:assert/strict";
import { test } from "node:test";
import { sameIdentity } from "./process-ownership.ts";
import type { ProcessIdentity } from "./evidence-models.ts";
import {
  identityRecord,
  observeOwned,
  parseProcessTable,
  processOperations,
  type ProcessOperations,
} from "./runtime-processes.ts";

// Literal `ps -o pid=,ppid=,ruid=,uid=,pgid=,lstart=,comm=` rows: real UID, then effective UID.
const ME = process.getuid?.() ?? 501;

const BIRTH = "Mon Oct 26 10:20:30 2026";

function tableRow(
  pid: number,
  parent: number,
  realUid: number,
  effectiveUid: number,
  comm: string,
): string {
  return `${pid} ${parent} ${realUid} ${effectiveUid} 10 ${BIRTH} ${comm}`;
}

function guardOperations(rows: readonly string[], alive: ReadonlySet<number>): ProcessOperations {
  const table = parseProcessTable(rows.join("\n"));
  return {
    ...processOperations,
    table: () => table,
    lookup: (pid: number) => {
      const identity = table.get(pid);
      return alive.has(pid) && identity !== undefined ? identityRecord(identity) : null;
    },
  };
}

test("the table parser keeps real and effective UID as separate fields", (): void => {
  assert.deepEqual(parseProcessTable(tableRow(40, 10, ME, 0, "/bin/ps")).get(40), {
    pid: 40,
    parent: 10,
    realUid: ME,
    effectiveUid: 0,
    group: 10,
    birth: BIRTH,
    executable: "/bin/ps",
  });
});

test("an owned setuid child is admitted by its real UID while a foreign real user is refused", (): void => {
  const ownRoot = tableRow(10, 1, ME, ME, "/owned/router");
  const setuidChild = tableRow(40, 10, ME, 0, "/bin/ps");
  const admitted = new Map<number, ProcessIdentity>();
  observeOwned(10, admitted, guardOperations([ownRoot, setuidChild], new Set([10, 40])));
  assert.deepEqual([...admitted.keys()].toSorted(), [10, 40]);
  assert.equal(admitted.get(40)?.effectiveUid, 0);

  // A different real user whose effective UID resembles the controller is still foreign.
  const foreign = tableRow(41, 10, ME + 1, ME, "/foreign/helper");
  assert.throws(
    () =>
      observeOwned(
        10,
        new Map<number, ProcessIdentity>(),
        guardOperations([ownRoot, foreign], new Set([10, 41])),
      ),
    /owned process UID changed/,
  );
  // The same foreign candidate gone at the fresh lookup is skipped, never admitted.
  const skipped = new Map<number, ProcessIdentity>();
  observeOwned(10, skipped, guardOperations([ownRoot, foreign], new Set([10])));
  assert.deepEqual([...skipped.keys()], [10]);
});

test("PID reuse is never adopted, while an effective-only transition keeps the identity", (): void => {
  const ownRoot = tableRow(10, 1, ME, ME, "/owned/router");
  const recordedChild = parseProcessTable(tableRow(11, 10, ME, ME, "/owned/provider")).get(11);
  if (recordedChild === undefined) throw new Error("fixture row did not parse");
  for (const reused of [
    `11 10 ${ME} ${ME} 10 Tue Oct 27 10:20:30 2026 /owned/provider`,
    `11 10 ${ME} ${ME} 99 ${BIRTH} /owned/provider`,
    `11 10 ${ME} ${ME} 10 ${BIRTH} /other/executable`,
  ]) {
    const known: Map<number, ProcessIdentity> = new Map([[11, recordedChild]]);
    observeOwned(10, known, guardOperations([ownRoot, reused], new Set([10, 11])));
    assert.deepEqual(known.get(11), recordedChild, reused);
  }
  // Intentional change: effective UID is evidence, not identity, so a setuid transition in place
  // no longer reads as a different process. A real-UID change still does.
  assert.equal(
    sameIdentity(
      { ...identityRecord(recordedChild), effectiveUid: 0 },
      identityRecord(recordedChild),
    ),
    true,
  );
  assert.equal(
    sameIdentity(
      { ...identityRecord(recordedChild), realUid: ME + 1 },
      identityRecord(recordedChild),
    ),
    false,
  );
});
