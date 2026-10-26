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
import {
  qualifyGlobalClaude,
  qualifyNormalBinaries,
  resolveGlobalClaude,
} from "../runtime-qualification.ts";

const rootIdentity = {
  pid: 10,
  parent: 1,
  uid: 501,
  group: 10,
  birth: "root-birth",
  executable: "/owned/router",
};
const childIdentity = {
  pid: 11,
  parent: 10,
  uid: 501,
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
      stdout: " 123 501 123 Mon Oct 26 10:20:30 2026 /owned/with  spaces/router\n",
      stderr: "",
    }),
    {
      pid: 123,
      uid: 501,
      group: 123,
      birth: "Mon Oct 26 10:20:30 2026",
      executable: "/owned/with  spaces/router",
    },
  );
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
