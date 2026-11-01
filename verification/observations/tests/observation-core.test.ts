import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { correlateMessage } from "../native-correlation.ts";
import {
  ownedClosure,
  parseProcessIdentity,
  ProcessIdentityError,
  remainingOwned,
  sameIdentity,
  signalEachOwned,
} from "../process-ownership.ts";
import type { ProcessIdentity } from "../evidence-models.ts";
import {
  identityRecord,
  observeOwned,
  parseProcessTable,
  processOperations,
  type ProcessOperations,
} from "../runtime-processes.ts";
import {
  qualifyGlobalClaude,
  qualifyNormalBinaries,
  resolveGlobalClaude,
} from "../runtime-qualification.ts";

const rootIdentity = {
  pid: 10,
  parent: 1,
  realUid: 501,
  effectiveUid: 501,
  group: 10,
  birth: "root-birth",
  executable: "/owned/router",
};
const childIdentity = {
  pid: 11,
  parent: 10,
  realUid: 501,
  effectiveUid: 501,
  group: 10,
  birth: "child-birth",
  executable: "/owned/provider",
};

test("recorded reparented children remain owned while reused roots cannot acquire descendants", (): void => {
  const known = new Map([
    [10, rootIdentity],
    [11, childIdentity],
  ]);
  assert.deepEqual(
    [...remainingOwned(known, new Map([[11, { ...childIdentity, parent: 1 }]])).keys()],
    [11],
  );
  assert.deepEqual(
    [
      ...ownedClosure(
        10,
        new Map([[10, rootIdentity]]),
        new Map([
          [10, { ...rootIdentity, birth: "new-birth" }],
          [12, { ...childIdentity, pid: 12 }],
        ]),
      ),
    ],
    [],
  );
  assert.equal(sameIdentity({ ...rootIdentity, parent: 9 }, rootIdentity), true);
  assert.throws(() => sameIdentity(null, {}), /incomplete recorded process identity/);
});

test("one disappeared child does not skip the remaining independently guarded signals", (): void => {
  const known = new Map([
    [10, rootIdentity],
    [11, childIdentity],
  ]);
  const signalled: number[] = [];
  const result = signalEachOwned(
    known,
    (pid) => known.get(pid) ?? null,
    (pid): void => {
      if (pid === 10) throw Object.assign(new Error("disappeared"), { code: "ESRCH" });
      signalled.push(pid);
    },
  );
  assert.deepEqual(signalled, [11]);
  assert.deepEqual(result, { sent: [11], errors: [] });
});

test("process query denial, absence and malformed snapshots remain distinct", (): void => {
  assert.equal(parseProcessIdentity({ status: 1, stdout: "", stderr: "" }), null);
  assert.throws(
    () => parseProcessIdentity({ status: 1, stdout: "", stderr: "denied" }),
    (error: unknown) =>
      error instanceof ProcessIdentityError && error.category === "CalledProcessError",
  );
  assert.throws(
    () => parseProcessIdentity({ status: 0, stdout: "malformed", stderr: "" }),
    /incomplete current process identity/,
  );
  assert.deepEqual(
    parseProcessIdentity({
      status: 0,
      stdout: " 123 501 0 123 Mon Oct 26 10:20:30 2026 /owned/with  spaces/router\n",
      stderr: "",
    }),
    {
      pid: 123,
      realUid: 501,
      effectiveUid: 0,
      group: 123,
      birth: "Mon Oct 26 10:20:30 2026",
      executable: "/owned/with  spaces/router",
    },
  );
});

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

test("a recorded identity from the effective-UID collector is refused, never reinterpreted", (): void => {
  const { realUid, effectiveUid, ...rest } = rootIdentity;
  assert.throws(
    () => sameIdentity(rootIdentity, { ...rest, uid: effectiveUid }),
    /incomplete recorded process identity/,
  );
  assert.equal(realUid, 501);
});

const correlationCases = z
  .array(
    z.object({ name: z.string(), thread: z.unknown(), expected: z.unknown(), result: z.unknown() }),
  )
  .parse(
    JSON.parse(readFileSync(new URL("baselines/native-correlation.json", import.meta.url), "utf8")),
  );
for (const entry of correlationCases)
  test(`native correlation matches original ${entry.name}`, (): void => {
    assert.deepEqual(correlateMessage(entry.thread, entry.expected), entry.result);
  });

test("native identities are strings and text content cannot be silently omitted", (): void => {
  assert.throws(() =>
    correlateMessage({ id: 123 }, { thread_id: "thread", turn_id: "turn", push_id: "push" }),
  );
  assert.throws(
    () =>
      correlateMessage(
        {
          id: "thread",
          turns: [
            {
              id: "turn",
              items: [
                { id: "item", type: "userMessage", clientId: "push", content: [{ type: "text" }] },
              ],
            },
          ],
        },
        { thread_id: "thread", turn_id: "turn", push_id: "push" },
      ),
    /native text content missing/,
  );
});

function executable(directory: string): string {
  mkdirSync(directory, { recursive: true });
  const path = join(directory, "claude");
  writeFileSync(path, "owned qualification fixture; never executed");
  chmodSync(path, 0o700);
  return path;
}

test("global Claude qualification records the canonical executable and exact hash without executing it", (): void => {
  const root = createFixtureDirectory("global-qualification");
  try {
    const binary = executable(join(root, "global-bin"));
    const alias = join(root, "planned-claude");
    symlinkSync(binary, alias);
    assert.deepEqual(qualifyGlobalClaude(alias, join(root, "global-bin")), {
      canonicalPath: realpathSync(binary),
      sha256: createHash("sha256")
        .update("owned qualification fixture; never executed")
        .digest("hex"),
    });
    const packageFile = executable(join(root, "project/node_modules/.bin"));
    symlinkSync(join(root, "project/node_modules/.bin"), join(root, "package-alias"));
    assert.equal(
      resolveGlobalClaude(
        [
          "relative/bin",
          join(root, "project/node_modules/.bin"),
          join(root, "package-alias"),
          join(root, "global-bin"),
        ].join(":"),
      ),
      realpathSync(binary),
    );
    assert.throws(
      () => qualifyGlobalClaude(packageFile, join(root, "global-bin")),
      /differs from qualified Host PATH/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("transient and SDK Claude aliases are rejected while the ordinary global package target is allowed", (): void => {
  const root = createFixtureDirectory("global-aliases");
  try {
    for (const [index, path] of [
      "_npx/fixture",
      "dlx/fixture",
      "node_modules/claude-agent-sdk-darwin-arm64",
      "node_modules/@anthropic-ai+claude-agent-sdk-fixture",
    ].entries()) {
      const target = executable(join(root, path));
      const directory = join(root, `alias-${index}`);
      mkdirSync(directory);
      symlinkSync(target, join(directory, "claude"));
      assert.throws(() => resolveGlobalClaude(directory), /unavailable on Host PATH/);
    }
    const global = executable(join(root, "global/lib/node_modules/@anthropic-ai/claude-code/bin"));
    const bin = join(root, "global/bin");
    mkdirSync(bin);
    symlinkSync(global, join(bin, "claude"));
    assert.equal(resolveGlobalClaude(bin), realpathSync(global));
    chmodSync(global, 0o600);
    assert.throws(() => resolveGlobalClaude(bin), /unavailable on Host PATH/);
    mkdirSync(join(root, "directory/claude"), { recursive: true });
    assert.throws(() => resolveGlobalClaude(join(root, "directory")), /unavailable on Host PATH/);
    for (const search of [undefined, "", "relative/bin"])
      assert.throws(() => resolveGlobalClaude(search), /unavailable on Host PATH/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("normal binary qualification checks hashes, test features and debug identity against a codesign probe", (): void => {
  const root = createFixtureDirectory("binary-qualification");
  try {
    const binaries = ["codex-router", "agent-collaboration", "agent-sessions"].map((target) => {
      const path = join(root, target);
      writeFileSync(path, target);
      return {
        target,
        path,
        sha256: createHash("sha256").update(target).digest("hex"),
        testGuardFeatures: [],
      };
    });
    const qualification = join(root, "qualification.json");
    writeFileSync(qualification, JSON.stringify({ binaries }));
    const probe = (path: string): string =>
      `Identifier=dev.shravansunder.${path.split("/").at(-1)}.debug\nTeamIdentifier=974QD84WVC\n`;
    assert.equal(qualifyNormalBinaries(qualification, probe).size, 3);
    assert.throws(
      () => qualifyNormalBinaries(qualification, () => "Identifier=wrong\n"),
      /debug signing identity mismatch/,
    );
    writeFileSync(join(root, "codex-router"), "changed bytes");
    assert.throws(
      () => qualifyNormalBinaries(qualification, probe),
      /binary hash or normal-build mismatch/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
