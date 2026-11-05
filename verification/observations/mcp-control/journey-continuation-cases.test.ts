// The two-lifetime continuation profile through the real case entry against two scripted Host
// lifetimes. Lifetime one creates one Session, completes one prompt, observes resume and close
// refused before dispatch and writes the exclusive continuation record. Lifetime two, a new
// epoch on the same service, validates that record and the same launch, sees the Session
// unloaded, loads it, finds the recorded history and completes one new prompt. The stand-in is
// never provider proof; it decides whether the driver refuses each broken continuity.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { decode } from "../../exact-json.ts";
import type { ControlPlan } from "./control-plan.ts";
import type { CaseResult } from "./journey-cases.ts";
import {
  standInPlan,
  startStandIn,
  writeHostContext,
  type RecordedExchange,
  type StandInOptions,
} from "./journey-stand-in.ts";
import { ProviderStoreReadback } from "./role-readback.ts";
import { runCases } from "./run.ts";
import { StreamingMcpClient } from "./streaming-mcp-client.ts";

const SECOND_EPOCH = "33333333-3333-4333-8333-333333333333";
const sha = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");

interface Lifetime {
  readonly result: CaseResult;
  readonly calls: readonly string[];
  readonly record: Buffer | null;
}

async function lifetime(
  options: StandInOptions,
  props: {
    /** The continuation's own stable context: its working directory and Router, both lifetimes. */
    readonly root: string;
    readonly cases: readonly string[];
    readonly continuation: (evidence: string) => unknown;
    readonly epoch?: string;
    readonly launchArguments: readonly string[];
    readonly recordPath?: (evidence: string) => string;
  },
): Promise<Lifetime> {
  const standIn = await startStandIn(options);
  try {
    const hostContext = join(standIn.root, "debug-host-context.json");
    writeHostContext(hostContext, {
      hostPid: process.pid,
      executable: process.execPath,
      arguments: props.launchArguments,
    });
    const evidence = join(standIn.root, "evidence");
    const plan = standInPlan(standIn, props.cases, {
      hostContext,
      expectedServiceEpoch: props.epoch ?? "22222222-2222-4222-8222-222222222222",
      continuation: props.continuation(evidence),
      workingDirectory: props.root,
      router: { path: props.root, sha256: "0".repeat(64) },
    }) as unknown as ControlPlan;
    mkdirSync(evidence);
    const store = new ProviderStoreReadback(standIn.store);
    try {
      const context = {
        client: new StreamingMcpClient(standIn.url),
        evidence,
        deadline: Date.parse(plan.deadline),
      };
      const [result] = await runCases(plan, context, store);
      if (result === undefined) throw new Error("no continuation result");
      let record: Buffer | null = null;
      try {
        record = props.recordPath === undefined ? null : readFileSync(props.recordPath(evidence));
      } catch {
        record = null;
      }
      return { result, calls: [...standIn.calls], record };
    } finally {
      store.close();
    }
  } finally {
    await standIn.close();
  }
}

/** The provider launch both lifetimes declare: an executable and one launched script file. */
function launchFile(root: string, content: string): string {
  const file = join(root, "provider-launch.ts");
  writeFileSync(file, content);
  return file;
}

async function retain(root: string, options: StandInOptions = {}): Promise<Lifetime> {
  return lifetime(
    { unsupported: ["resume", "close"], ...options },
    {
      root,
      cases: ["continuationRetain"],
      continuation: (evidence) => ({
        phase: "retain",
        record: join(evidence, "continuation-record.json"),
      }),
      launchArguments: [launchFile(root, "provider launch v1")],
      recordPath: (evidence) => join(evidence, "continuation-record.json"),
    },
  );
}

function reloadExisting(record: Buffer): StandInOptions {
  const parsed = decode(record.toString("utf8")) as {
    target: { sessionId: string };
    history: RecordedExchange[];
  };
  return {
    unsupported: ["resume", "close"],
    bindingEpoch: SECOND_EPOCH,
    existing: [{ sessionId: parsed.target.sessionId, history: parsed.history }],
  };
}

async function reload(
  root: string,
  record: Buffer,
  props: {
    readonly options?: StandInOptions;
    readonly recordBytes?: Buffer;
    readonly recordSha256?: string;
    readonly epoch?: string;
    readonly launch?: string;
  } = {},
): Promise<Lifetime> {
  const recordPath = join(root, "kept-continuation-record.json");
  writeFileSync(recordPath, props.recordBytes ?? record);
  return lifetime(
    { ...reloadExisting(record), ...props.options },
    {
      root,
      cases: ["continuationReload"],
      epoch: props.epoch ?? SECOND_EPOCH,
      continuation: () => ({
        phase: "reload",
        record: recordPath,
        recordSha256: props.recordSha256 ?? sha(props.recordBytes ?? record),
      }),
      launchArguments: [launchFile(root, props.launch ?? "provider launch v1")],
    },
  );
}

async function withRoot(body: (root: string) => Promise<void>): Promise<void> {
  const root = mkdtempSync(join(tmpdir(), "mcp-continuation-"));
  try {
    await body(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("lifetime one records one prompt and two refused-before-dispatch controls, then lifetime two continues it", async () => {
  await withRoot(async (root) => {
    const first = await retain(root);
    assert.equal(first.result.verdict.kind, "qualified", JSON.stringify(first.result.verdict));
    assert.ok(first.record !== null, "the continuation record was not written");
    const record = decode(first.record.toString("utf8")) as Record<string, unknown>;
    assert.equal((record["history"] as unknown[]).length, 1);
    assert.equal(first.calls.filter((tool) => tool === "conversation_create").length, 1);
    const second = await reload(root, first.record);
    assert.equal(second.result.verdict.kind, "qualified", JSON.stringify(second.result.verdict));
    assert.equal(second.calls.includes("conversation_create"), false, "reload created a Session");
    assert.ok(
      second.calls.indexOf("provider_session_inspect") < second.calls.indexOf("conversation_load"),
      "the unloaded state was not read before the load",
    );
  });
});

test("lifetime one refuses a supported resume or close and a refused operation with a store record", async () => {
  await withRoot(async (root) => {
    assert.equal((await retain(root, { unsupported: ["close"] })).result.verdict.kind, "failed");
    assert.equal((await retain(root, { recordRefused: true })).result.verdict.kind, "failed");
  });
});

test("lifetime two refuses each broken continuity before it sends the load", async () => {
  await withRoot(async (root) => {
    const first = await retain(root);
    assert.ok(first.record !== null);
    const broken: [string, Lifetime][] = [
      ["already attached", await reload(root, first.record, { options: { sessionState: "idle" } })],
      [
        "reused epoch",
        await reload(root, first.record, { epoch: "22222222-2222-4222-8222-222222222222" }),
      ],
      ["changed launch", await reload(root, first.record, { launch: "provider launch v2" })],
      ["record hash mismatch", await reload(root, first.record, { recordSha256: "0".repeat(64) })],
      [
        "fresh provider Session",
        await reload(root, first.record, {
          recordBytes: Buffer.from(first.record.toString("utf8").replace("session-1", "session-9")),
        }),
      ],
    ];
    for (const [name, outcome] of broken) {
      assert.equal(
        outcome.result.verdict.kind,
        "failed",
        `${name}: ${JSON.stringify(outcome.result.verdict)}`,
      );
      assert.equal(outcome.calls.includes("conversation_load"), false, `${name} sent the load`);
    }
  });
});

test("lifetime two refuses canned or missing history after a real load", async () => {
  await withRoot(async (root) => {
    const first = await retain(root);
    assert.ok(first.record !== null);
    for (const loadReplay of ["canned", "none"] as const) {
      const outcome = await reload(root, first.record, { options: { loadReplay } });
      assert.equal(outcome.result.verdict.kind, "failed", loadReplay);
    }
  });
});
