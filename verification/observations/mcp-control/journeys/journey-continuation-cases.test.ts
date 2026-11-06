// The two-lifetime continuation profile through the real case entry against two scripted Host
// lifetimes over one prepared context, laid out as the automation Host lays it out. Lifetime
// one creates one Session, completes one prompt, observes resume and close refused before
// dispatch and writes the exclusive continuation record. Lifetime two, a new epoch on the same
// service and context, validates that record, the same prepared context and launch, the live
// binding and the unloaded Session, loads it, finds the recorded history and completes one new
// prompt. The stand-in is never provider proof; it decides whether the driver refuses each
// broken continuity.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { decode } from "../../../exact-json.ts";
import { continuationRecordSchema } from "../admission/continuation-record.ts";
import type { CaseResult } from "./journey-cases.ts";
import {
  STAND_IN_EPOCH,
  startStandIn,
  type StandInOptions,
} from "../test-support/journey-stand-in.ts";
import {
  preparedLayout,
  standInPlan,
  writeHostContext,
} from "../test-support/journey-stand-in-plan.ts";
import { ProviderStoreReadback } from "../evidence/role-readback.ts";
import { runCases } from "../run.ts";
import { StreamingMcpClient } from "../transport/streaming-mcp-client.ts";

const SECOND_EPOCH = "33333333-3333-4333-8333-333333333333";
const sha = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");

interface Lifetime {
  readonly result: CaseResult;
  readonly calls: readonly string[];
  readonly record: Buffer | null;
}

/** What one lifetime's prepared context declares, beyond the shared layout. */
interface ContextChange {
  readonly profile?: string;
  readonly model?: string;
  readonly port?: number;
  readonly peerRegistryDirectory?: string | null;
}

async function lifetime(
  options: StandInOptions,
  props: {
    /** The continuation's stable root: one prepared context across both lifetimes. */
    readonly root: string;
    readonly cases: readonly string[];
    readonly continuation: (evidence: string) => unknown;
    readonly epoch: string;
    readonly launchArguments: readonly string[];
    readonly context?: ContextChange;
    readonly recordPath?: (evidence: string) => string;
  },
): Promise<Lifetime> {
  const standIn = await startStandIn(options);
  try {
    const layout = preparedLayout(props.root, props.epoch);
    writeHostContext(layout, {
      hostPid: process.pid,
      executable: process.execPath,
      arguments: props.launchArguments,
      ...props.context,
    });
    const evidence = join(standIn.root, "evidence");
    const plan = standInPlan(standIn, props.cases, {
      hostContext: layout.hostContext,
      serviceDirectory: layout.serviceDirectory,
      workingDirectory: layout.workspace,
      expectedServiceEpoch: props.epoch,
      continuation: props.continuation(evidence),
      router: { path: props.root, sha256: "0".repeat(64) },
    });
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
      const recordPath = props.recordPath?.(evidence);
      let record: Buffer | null = null;
      try {
        record = recordPath === undefined ? null : readFileSync(recordPath);
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
      epoch: STAND_IN_EPOCH,
      continuation: (evidence) => ({
        phase: "retain",
        record: join(evidence, "continuation-record.json"),
      }),
      launchArguments: [launchFile(root, "provider launch v1")],
      recordPath: (evidence) => join(evidence, "continuation-record.json"),
    },
  );
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
    readonly context?: ContextChange;
    readonly createdBy?: string;
  } = {},
): Promise<Lifetime> {
  const kept = continuationRecordSchema.parse(decode(record.toString("utf8")));
  const recordPath = join(root, "kept-continuation-record.json");
  writeFileSync(recordPath, props.recordBytes ?? record);
  return lifetime(
    {
      unsupported: ["resume", "close"],
      bindingEpoch: props.epoch ?? SECOND_EPOCH,
      existing: [
        {
          sessionId: kept.target.sessionId,
          history: kept.history,
          ...(props.createdBy === undefined ? {} : { createdBy: props.createdBy }),
        },
      ],
      ...props.options,
    },
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
      ...(props.context === undefined ? {} : { context: props.context }),
    },
  );
}

async function withRetained(body: (root: string, record: Buffer) => Promise<void>): Promise<void> {
  const root = mkdtempSync(join(tmpdir(), "mcp-continuation-"));
  try {
    const first = await retain(root);
    assert.equal(first.result.verdict.kind, "qualified", JSON.stringify(first.result.verdict));
    if (first.record === null) throw new Error("the continuation record was not written");
    await body(root, first.record);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("lifetime one records one prompt and two refused-before-dispatch controls, then lifetime two continues it", async () => {
  await withRetained(async (root, record) => {
    const kept = continuationRecordSchema.parse(decode(record.toString("utf8")));
    assert.equal(kept.history.length, 1);
    assert.equal(kept.context.routerProxyPort, 18791);
    const second = await reload(root, record);
    assert.equal(second.result.verdict.kind, "qualified", JSON.stringify(second.result.verdict));
    assert.equal(second.calls.includes("conversation_create"), false, "reload created a Session");
    const order = ["provider_session_inspect", "endpoints_list", "conversation_load"].map((tool) =>
      second.calls.indexOf(tool),
    );
    assert.ok(
      order.every((index) => index >= 0),
      "a pre-load read or the load is missing",
    );
    assert.deepEqual(
      [...order].sort((a, b) => a - b),
      order,
      `pre-load order: ${order.join(",")}`,
    );
  });
});

test("lifetime one refuses a supported resume or close and a refused operation with a store record", async () => {
  const root = mkdtempSync(join(tmpdir(), "mcp-continuation-"));
  try {
    assert.equal((await retain(root, { unsupported: ["close"] })).result.verdict.kind, "failed");
    assert.equal((await retain(root, { recordRefused: true })).result.verdict.kind, "failed");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("lifetime two refuses each broken record, role, context, launch or live binding before it sends the load", async () => {
  await withRetained(async (root, record) => {
    const broken: [string, Lifetime][] = [
      ["already attached", await reload(root, record, { options: { sessionState: "idle" } })],
      [
        "inspect names another Session",
        await reload(root, record, { options: { inspectSessionId: "session-9" } }),
      ],
      ["reused epoch", await reload(root, record, { epoch: STAND_IN_EPOCH })],
      ["changed launch", await reload(root, record, { launch: "provider launch v2" })],
      ["record hash mismatch", await reload(root, record, { recordSha256: "0".repeat(64) })],
      [
        "fresh provider Session",
        await reload(root, record, {
          recordBytes: Buffer.from(record.toString("utf8").replace("session-1", "session-9")),
        }),
      ],
      ["changed stored creator", await reload(root, record, { createdBy: "someone-else" })],
      ["changed profile", await reload(root, record, { context: { profile: "other-profile" } })],
      ["changed native model", await reload(root, record, { context: { model: "other-model" } })],
      [
        "changed peer registry",
        await reload(root, record, { context: { peerRegistryDirectory: join(root, "registry") } }),
      ],
      ["port not the Router proxy", await reload(root, record, { context: { port: 51999 } })],
      [
        "inventory from another epoch",
        await reload(root, record, { options: { inventory: { epoch: STAND_IN_EPOCH } } }),
      ],
      [
        "endpoint unavailable",
        await reload(root, record, { options: { inventory: { unavailable: true } } }),
      ],
      [
        "no provider binding",
        await reload(root, record, { options: { inventory: { withoutProviderChannel: true } } }),
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

test("lifetime two refuses a load bound to a stale epoch or to another live binding", async () => {
  await withRetained(async (root, record) => {
    for (const [name, loadBinding] of [
      ["stale binding epoch", { epoch: STAND_IN_EPOCH }],
      ["other binding", { bindingId: "33333333-0000-4000-8000-0000000000ff" }],
    ] as const) {
      const outcome = await reload(root, record, { options: { loadBinding } });
      assert.equal(outcome.result.verdict.kind, "failed", name);
      assert.match(
        outcome.result.verdict.kind === "failed" ? outcome.result.verdict.reason : "",
        /binding/,
        name,
      );
    }
  });
});

test("lifetime two refuses canned or missing history after a real load", async () => {
  await withRetained(async (root, record) => {
    for (const loadReplay of ["canned", "none"] as const) {
      const outcome = await reload(root, record, { options: { loadReplay } });
      assert.equal(outcome.result.verdict.kind, "failed", loadReplay);
    }
  });
});
