import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";
import { sameIdentity } from "./process-ownership.ts";
import {
  identityRecordSchema,
  processIdentitySchema,
  type ProcessIdentity,
} from "../shared/evidence-models.ts";
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

// Caller locale is fixed per child process, so no test mutates this process's environment.
function callerEnvironment(locale: Readonly<Record<string, string>>): NodeJS.ProcessEnv {
  const inherited = Object.entries(process.env).filter(
    ([name]) => name !== "LANG" && !name.startsWith("LC_"),
  );
  return { ...Object.fromEntries(inherited), ...locale };
}

const C_CALLER = callerEnvironment({ LC_ALL: "C" });
const EN_CA_CALLER = callerEnvironment({ LANG: "en_CA.UTF-8" });

function rawStartTime(environment: NodeJS.ProcessEnv): string {
  const result = spawnSync("/bin/ps", ["-p", String(process.pid), "-o", "lstart="], {
    encoding: "utf8",
    env: environment,
    timeout: 5000,
  });
  if (result.error !== undefined || result.status !== 0)
    throw new Error(
      `ps could not read this test's own start time: ${result.error?.message ?? result.stderr.trim()}`,
    );
  return result.stdout.trim().split(/\s+/).join(" ");
}

// A host whose ps prints one start-time format for every locale cannot reach this case.
function hostPsIgnoresCallerLocale(): string | false {
  return rawStartTime(C_CALLER) === rawStartTime(EN_CA_CALLER)
    ? "host ps prints lstart identically under C and en_CA.UTF-8, so caller locale cannot reach a recorded birth"
    : false;
}

const readingSchema = z.object({ table: processIdentitySchema, lookup: identityRecordSchema });
const READER_SCRIPT = [
  "const { processTable, identityRecord } = await import(process.argv[1]);",
  "const { lookupProcessIdentity } = await import(process.argv[2]);",
  "const pid = Number(process.argv[3]);",
  "const row = processTable().get(pid);",
  "process.stdout.write(JSON.stringify({ table: row && identityRecord(row), lookup: lookupProcessIdentity(pid) }));",
].join("\n");

/** Reads this live test process through both real readers from a caller with `environment`. */
function readThisProcessAs(environment: NodeJS.ProcessEnv): z.infer<typeof readingSchema> {
  const result = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      READER_SCRIPT,
      new URL("./runtime-processes.ts", import.meta.url).href,
      new URL("./process-ownership.ts", import.meta.url).href,
      String(process.pid),
    ],
    { encoding: "utf8", env: environment, timeout: 15_000 },
  );
  assert.equal(result.status, 0, result.stderr);
  return readingSchema.parse(JSON.parse(result.stdout));
}

test(
  "a live process keeps one exact identity across capture and recheck whatever the caller's locale",
  { timeout: 30_000, skip: hostPsIgnoresCallerLocale() },
  (): void => {
    const fromC = readThisProcessAs(C_CALLER);
    const fromEnCa = readThisProcessAs(EN_CA_CALLER);
    assert.deepEqual(fromEnCa.table, fromC.table);
    // Captured by the table under one locale and rechecked by lookup under the other, the exact
    // guard still matches the same live process.
    assert.equal(sameIdentity(fromEnCa.lookup, identityRecord(fromC.table)), true);
    assert.equal(sameIdentity(fromC.lookup, identityRecord(fromEnCa.table)), true);
  },
);
