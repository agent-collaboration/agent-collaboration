// --check sensitivity on a disposable repository, service directory and synthetic schema source
// generated from the inventory itself: the correct plan passes, and each wrong declaration is
// refused before anything would contact a Host.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { loadAcpInventory } from "./acp-inventory.ts";
import {
  checkControlPlan,
  controlPlanSchema,
  isProductionRouterPath,
  type ControlPlan,
} from "./control-plan.ts";

const NOW = new Date("2026-11-05T02:00:00Z");
const sha = (text: string | Buffer): string => createHash("sha256").update(text).digest("hex");
// The declared Host is this test process: a live process whose executable is the artifact, so the
// process binding is real without launching anything.
const HOST_PATH = realpathSync(process.execPath);
const HOST_SHA = sha(readFileSync(HOST_PATH));

/** A schema source whose constants and SessionUpdate variants are exactly the inventory's. */
function writeSchemaSource(directory: string): void {
  const inventory = loadAcpInventory();
  const v1 = join(directory, "src", "v1");
  mkdirSync(v1, { recursive: true });
  const constants = inventory.rows
    .filter((row) => row.kind === "method" || row.kind === "notification")
    .map((row, index) => {
      const suffix =
        row.kind === "notification" && !row.name.includes("cancel")
          ? "NOTIFICATION"
          : "METHOD_NAME";
      const gate = row.gate === null ? "" : `#[cfg(feature = "${row.gate}")]\n`;
      return `${gate}pub(crate) const NAME_${index}_${suffix}: &str = "${row.name}";`;
    });
  const variants = inventory.rows
    .filter((row) => row.kind === "sessionUpdate")
    .map((row) => {
      const name = row.name.replace(
        /(^|_)([a-z])/g,
        (_match, _start: string, letter: string): string => letter.toUpperCase(),
      );
      return `${row.gate === null ? "" : `    #[cfg(feature = "${row.gate}")]\n`}    ${name}(${name}),`;
    });
  writeFileSync(join(v1, "agent.rs"), `${constants.join("\n")}\n`);
  writeFileSync(join(v1, "client.rs"), `pub enum SessionUpdate {\n${variants.join("\n")}\n}\n`);
}

function fixture(root: string): ControlPlan {
  const repository = join(root, "repo");
  mkdirSync(repository);
  const router = join(root, "codex-router");
  writeFileSync(router, "router-binary");
  writeFileSync(
    join(repository, "Cargo.lock"),
    '[[package]]\nname = "agent-client-protocol-schema"\nversion = "1.9.1"\n',
  );
  const git = (...args: string[]): string =>
    execFileSync("git", ["-C", repository, ...args], { encoding: "utf8" }).trim();
  git("init", "-q");
  // A disposable repository: no signing prompt and no user hooks may run from a test.
  const hooks = join(root, "no-hooks");
  mkdirSync(hooks);
  const local = [
    "-c",
    "user.email=v@example.invalid",
    "-c",
    "user.name=v",
    "-c",
    "commit.gpgsign=false",
    "-c",
    `core.hooksPath=${hooks}`,
  ];
  git(...local, "add", "Cargo.lock");
  git(...local, "commit", "-qm", "fixture");
  const sourceHead = git("rev-parse", "HEAD");
  const manifest = join(root, "manifest.json");
  writeFileSync(
    manifest,
    JSON.stringify({
      source_commit: sourceHead,
      source_tree: "b".repeat(40),
      dirty: false,
      test_guard_features: [],
      binaries: {
        "automation-debug-host": { path: HOST_PATH, sha256: HOST_SHA, signature: {} },
        "codex-router": { path: router, sha256: sha("router-binary"), signature: {} },
      },
    }),
  );
  const service = join(root, "service");
  mkdirSync(service);
  writeFileSync(join(service, "provider-operations.sqlite"), "");
  const serviceId = "11111111-1111-4111-8111-111111111111";
  const serviceEpoch = "22222222-2222-4222-8222-222222222222";
  writeFileSync(
    join(service, "service.json"),
    JSON.stringify({ serviceId, serviceEpoch, mcp: { url: "http://127.0.0.1:51234/mcp" } }),
  );
  const schema = join(root, "schema");
  writeSchemaSource(schema);
  const owner = {
    kind: "human" as const,
    humanId: userInfo().username,
    source: "ownerLookup" as const,
  };
  return controlPlanSchema.parse({
    repository,
    sourceHead,
    buildManifest: manifest,
    host: { kind: "automationDebugHost", pid: process.pid, path: HOST_PATH, sha256: HOST_SHA },
    hostContext: null,
    router: { path: router, sha256: sha("router-binary") },
    serviceDirectory: service,
    expectedServiceId: serviceId,
    expectedServiceEpoch: serviceEpoch,
    endpointId: "cursor-local",
    workingDirectory: root,
    actor: owner,
    approver: owner,
    wrongRoleRequester: null,
    peer: {
      kind: "controlled",
      receipt: join(root, "receipt.jsonl"),
      releaseSentinel: join(root, "release"),
    },
    cases: ["lifecycle", "callerLoss"],
    promptText: "ordinary",
    holdText: "hold",
    effects: { maxSessions: 2, maxPrompts: 2 },
    deadline: "2026-11-05T03:00:00Z",
    evidence: join(root, "evidence"),
    schemaSource: schema,
    continuation: null,
  });
}

function withFixture(body: (plan: ControlPlan, root: string) => void): void {
  const root = mkdtempSync(join(tmpdir(), "mcp-control-plan-"));
  try {
    body(fixture(root), root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("a correct plan passes --check and names the manifest's MCP endpoint", () => {
  withFixture((plan) => {
    assert.deepEqual(checkControlPlan(plan, NOW), {
      mcpEndpoint: "http://127.0.0.1:51234/mcp",
      providerStore: join(plan.serviceDirectory, "provider-operations.sqlite"),
    });
  });
});

test("a wrong build, service, source, caller, budget, deadline or sentinel is refused", () => {
  withFixture((plan, root) => {
    const refusals: [Partial<ControlPlan>, RegExp][] = [
      [{ host: { ...plan.host, sha256: "0".repeat(64) } }, /hash differs/],
      [
        { expectedServiceEpoch: "33333333-3333-4333-8333-333333333333" },
        /service identity or epoch/,
      ],
      [{ sourceHead: "f".repeat(40) }, /not at the declared source head/],
      [
        { actor: { kind: "human", humanId: "someone-else", source: "ownerLookup" } },
        /not the owner lookup/,
      ],
      [{ effects: { maxSessions: 1, maxPrompts: 2 } }, /beyond the declared effects/],
      [{ deadline: "2026-11-05T01:00:00Z" }, /deadline has passed/],
      [{ holdText: null }, /need a declared hold text/],
    ];
    for (const [change, expected] of refusals)
      assert.throws(() => checkControlPlan({ ...plan, ...change }, NOW), expected);
    writeFileSync(join(root, "release"), "stale");
    assert.throws(() => checkControlPlan(plan, NOW), /release sentinel already exists/);
  });
});

test("an uncommitted change or a drifted locked schema is refused", () => {
  withFixture((plan, root) => {
    writeFileSync(join(plan.repository, "untracked.txt"), "x");
    assert.throws(() => checkControlPlan(plan, NOW), /uncommitted changes/);
    rmSync(join(plan.repository, "untracked.txt"));
    writeFileSync(
      join(root, "schema", "src", "v1", "extra.rs"),
      'pub(crate) const X_METHOD_NAME: &str = "session/archive";\n',
    );
    assert.throws(() => checkControlPlan(plan, NOW), /session\/archive has no inventory row/);
  });
});

test("only the exact production root and its subtree are refused; debug siblings are usable", () => {
  const home = mkdtempSync(join(tmpdir(), "router-home-"));
  try {
    mkdirSync(join(home, ".codex-router", "agent-communication"), { recursive: true });
    mkdirSync(join(home, ".codex-router-debug", "agent-communication"), { recursive: true });
    mkdirSync(join(home, ".codex-router-other"));
    symlinkSync(join(home, ".codex-router"), join(home, "linked-production"));
    assert.equal(isProductionRouterPath(join(home, ".codex-router"), home), true);
    assert.equal(
      isProductionRouterPath(join(home, ".codex-router", "agent-communication"), home),
      true,
    );
    assert.equal(
      isProductionRouterPath(join(home, "linked-production", "agent-communication"), home),
      true,
    );
    assert.equal(
      isProductionRouterPath(join(home, ".codex-router-debug", "agent-communication"), home),
      false,
    );
    assert.equal(isProductionRouterPath(join(home, ".codex-router-other"), home), false);
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
});

test("an MCP endpoint on a protected production port is refused", () => {
  withFixture((plan) => {
    writeFileSync(
      join(plan.serviceDirectory, "service.json"),
      JSON.stringify({
        serviceId: plan.expectedServiceId,
        serviceEpoch: plan.expectedServiceEpoch,
        mcp: { url: "http://127.0.0.1:19741/mcp" },
      }),
    );
    assert.throws(() => checkControlPlan(plan, NOW), /protected production port/);
  });
});

function rewriteManifest(
  plan: ControlPlan,
  change: (manifest: Record<string, unknown>) => void,
): void {
  const manifest = JSON.parse(readFileSync(plan.buildManifest, "utf8")) as Record<string, unknown>;
  change(manifest);
  writeFileSync(plan.buildManifest, JSON.stringify(manifest));
}

test("the Router and Host must both be the normal manifest's own artifacts", () => {
  withFixture((plan, root) => {
    const other = join(root, "other-router");
    writeFileSync(other, "unrelated-router");
    assert.throws(
      () =>
        checkControlPlan(
          { ...plan, router: { path: other, sha256: sha("unrelated-router") } },
          NOW,
        ),
      /Router is not the manifest's build/,
    );
    rewriteManifest(plan, (manifest) => {
      manifest["test_guard_features"] = ["keychain-test-support"];
    });
    assert.throws(() => checkControlPlan(plan, NOW), /not a normal build/);
  });
});

test("the declared Host process must be running the declared Host artifact", () => {
  withFixture((plan) => {
    assert.throws(
      () => checkControlPlan({ ...plan, host: { ...plan.host, pid: 1 } }, NOW),
      /Host process/,
    );
  });
});

test("a foreground Router Host is the manifest's codex-router artifact, selected explicitly", () => {
  withFixture((plan) => {
    rewriteManifest(plan, (manifest) => {
      manifest["binaries"] = {
        "automation-debug-host": {
          path: plan.router.path,
          sha256: plan.router.sha256,
          signature: {},
        },
        "codex-router": { path: HOST_PATH, sha256: HOST_SHA, signature: {} },
      };
    });
    const foreground: ControlPlan = {
      ...plan,
      host: { ...plan.host, kind: "foregroundRouterHost" },
      router: { path: HOST_PATH, sha256: HOST_SHA },
    };
    assert.doesNotThrow(() => checkControlPlan(foreground, NOW));
    assert.throws(
      () => checkControlPlan({ ...foreground, host: { ...plan.host } }, NOW),
      /Host is not the manifest's build/,
    );
  });
});

test("a real provider cannot run the held caller-loss or explicit-cancel cases", () => {
  withFixture((plan) => {
    for (const held of ["callerLoss", "explicitCancel"] as const)
      assert.throws(
        () =>
          checkControlPlan(
            { ...plan, peer: { kind: "provider" }, cases: [held], holdText: "hold" },
            NOW,
          ),
        /actual causal hold/,
      );
  });
});

/** A continuation record in the shape continuationRetain writes, bound to `plan`'s first lifetime. */
function continuationRecord(plan: ControlPlan, launch: unknown): Record<string, unknown> {
  return {
    kind: "mcpControlContinuation",
    version: 1,
    target: {
      endpoint: { serviceId: plan.expectedServiceId, endpointId: plan.endpointId },
      sessionId: "session-1",
    },
    serviceEpoch: "44444444-4444-4444-8444-444444444444",
    workingDirectory: plan.workingDirectory,
    host: { kind: plan.host.kind, path: plan.host.path, sha256: plan.host.sha256 },
    router: plan.router,
    launch,
    actor: plan.actor,
    approver: plan.approver,
    storedRoles: { createdBy: { humanId: "owner" }, approver: { humanId: "owner" } },
    history: [{ prompt: "ordinary", reply: "beforeafter" }],
    operations: { create: "c", prompt: "p", unsupportedResume: "r", unsupportedClose: "x" },
  };
}

function continuationFixture(
  plan: ControlPlan,
  root: string,
): {
  readonly hostContext: string;
  readonly launch: unknown;
} {
  const script = join(root, "provider-launch.ts");
  writeFileSync(script, "launch v1");
  const hostContext = join(root, "debug-host-context.json");
  writeFileSync(
    hostContext,
    JSON.stringify({
      kind: "debugHostPrepared",
      hostPid: plan.host.pid,
      claudeProvider: null,
      cursorProvider: { executable: HOST_PATH, arguments: [script, "--flag"] },
    }),
  );
  return {
    hostContext,
    launch: {
      executable: HOST_PATH,
      executableSha256: HOST_SHA,
      arguments: [
        { value: script, sha256: sha("launch v1") },
        { value: "--flag", sha256: null },
      ],
    },
  };
}

test("a retain continuation needs its own lone case, the Host context and an absent record in its evidence", () => {
  withFixture((plan, root) => {
    const { hostContext } = continuationFixture(plan, root);
    const retain: ControlPlan = {
      ...plan,
      cases: ["continuationRetain"],
      hostContext,
      continuation: { phase: "retain", record: join(plan.evidence, "continuation-record.json") },
    };
    assert.doesNotThrow(() => checkControlPlan(retain, NOW));
    const refusals: [Partial<ControlPlan>, RegExp][] = [
      [{ hostContext: null }, /Host context/],
      [{ cases: ["continuationRetain", "lifecycle"] }, /only case/],
      [{ continuation: null }, /continuation declaration/],
      [
        { continuation: { phase: "retain", record: join(root, "elsewhere.json") } },
        /inside the evidence/,
      ],
      [{ host: { ...plan.host, kind: "foregroundRouterHost" } }, /automation Host/],
    ];
    for (const [change, expected] of refusals)
      assert.throws(() => checkControlPlan({ ...retain, ...change }, NOW), expected);
    writeFileSync(
      hostContext,
      JSON.stringify({ kind: "debugHostPrepared", hostPid: 1, cursorProvider: null }),
    );
    assert.throws(() => checkControlPlan(retain, NOW), /Host context/);
  });
});

test("a reload continuation validates the exact kept record before any effect", () => {
  withFixture((plan, root) => {
    const { hostContext, launch } = continuationFixture(plan, root);
    const recordPath = join(root, "kept-record.json");
    const write = (record: Record<string, unknown>): string => {
      const bytes = JSON.stringify(record);
      writeFileSync(recordPath, bytes);
      return sha(bytes);
    };
    const base: ControlPlan = {
      ...plan,
      cases: ["continuationReload"],
      hostContext,
      continuation: {
        phase: "reload",
        record: recordPath,
        recordSha256: write(continuationRecord(plan, launch)),
      },
    };
    assert.doesNotThrow(() => checkControlPlan(base, NOW));
    const refusals: [Record<string, unknown>, RegExp][] = [
      [{ serviceEpoch: plan.expectedServiceEpoch }, /new service epoch/],
      [
        {
          target: {
            endpoint: {
              serviceId: "99999999-9999-4999-8999-999999999999",
              endpointId: plan.endpointId,
            },
            sessionId: "session-1",
          },
        },
        /same service and endpoint/,
      ],
      [{ workingDirectory: join(root, "other") }, /working directory/],
      [
        { launch: { ...(launch as Record<string, unknown>), arguments: [] } },
        /same provider launch/,
      ],
      [{ router: { path: plan.router.path, sha256: "0".repeat(64) } }, /same Host and Router/],
    ];
    for (const [change, expected] of refusals) {
      const digest = write({ ...continuationRecord(plan, launch), ...change });
      assert.throws(
        () =>
          checkControlPlan(
            {
              ...base,
              continuation: { phase: "reload", record: recordPath, recordSha256: digest },
            },
            NOW,
          ),
        expected,
      );
    }
    write(continuationRecord(plan, launch));
    assert.throws(
      () =>
        checkControlPlan(
          {
            ...base,
            continuation: { phase: "reload", record: recordPath, recordSha256: "0".repeat(64) },
          },
          NOW,
        ),
      /record hash/,
    );
    writeFileSync(join(root, "provider-launch.ts"), "launch v2");
    assert.throws(() => checkControlPlan(base, NOW), /same provider launch/);
  });
});
