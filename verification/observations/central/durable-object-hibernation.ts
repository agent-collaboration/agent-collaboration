// Qualifies the lasting forced-eviction oracle on real local workerd stand-ins.
// --help states the absent C2 product target; a fixture pass never grants it.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomUUIDv7, createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  Miniflare,
  Response,
  convertV4MiniflareOptions,
  type MessageEvent,
  type V4FetchHandler,
} from "miniflare";
import { z } from "zod";
import { runC2ProductPlan } from "./c2-product-run.ts";
import {
  attachmentWitnessHeader,
  hibernationWorkerSource,
} from "../../test-support/hibernation-worker.ts";
import { constructMiniflareWithOwnedSignalShutdown } from "../../test-support/miniflare-signal-shutdown.ts";

const attachmentSchema = z.object({ witness: z.uuidv7() }).strict();
const welcomeSchema = z
  .object({ kind: z.literal("welcome"), instance: z.uuidv7(), attachment: attachmentSchema })
  .strict();
const observedSchema = z
  .object({
    kind: z.literal("observed"),
    instance: z.uuidv7(),
    attachment: attachmentSchema,
    count: z.number().int().positive(),
  })
  .strict();
const witnessEventSchema = z.object({ kind: z.enum(["constructor", "handler"]) }).strict();
const fixtureModeSchema = z.enum(["hibernating", "non-hibernating"]);
type FixtureMode = z.infer<typeof fixtureModeSchema>;
const witnessPhaseSchema = z.enum([
  "connecting",
  "baseline",
  "evicting",
  "heartbeat",
  "deciding",
  "settling",
]);
const constructorWitnessSchema = z
  .object({ instance: z.uuidv7(), phase: witnessPhaseSchema })
  .strict();
export type ConstructorWitness = Readonly<z.infer<typeof constructorWitnessSchema>>;

interface WitnessCounts {
  readonly constructors: number;
  readonly handlers: number;
}

export function assertHeartbeatDidNotWake(before: WitnessCounts, after: WitnessCounts): void {
  assert.deepEqual(
    after,
    before,
    "heartbeat must add no observed constructor or handler witnesses",
  );
}

export function assertEvictionDidNotReconstruct(before: WitnessCounts, after: WitnessCounts): void {
  assert.deepEqual(after, before, "eviction must run neither constructor nor handler");
}

export function assertSingleDecidingFrame(before: WitnessCounts, after: WitnessCounts): void {
  assert.deepEqual(
    after,
    { constructors: before.constructors + 1, handlers: before.handlers + 1 },
    "deciding frame must run exactly one constructor and handler",
  );
}

export function assertSingleWelcome(welcomes: number): void {
  assert.equal(welcomes, 1, "no reconnect or replacement Welcome");
}

export function assertDecidingConstructor(
  records: readonly ConstructorWitness[],
  instance: string,
): void {
  const matching = records.filter((record): boolean => record.instance === instance);
  assert.equal(matching.length, 1, "serving instance must have exactly one constructor witness");
  assert.equal(
    matching[0]?.phase,
    "deciding",
    "serving constructor witness must arrive after observe dispatch",
  );
}

type FixtureDisposition =
  | { readonly kind: "passed"; readonly exitCode: 0 }
  | { readonly kind: "expected-red"; readonly exitCode: 1 }
  | { readonly kind: "failed"; readonly exitCode: 1 }
  | { readonly kind: "invalid-control"; readonly exitCode: 2 };

export function classifyFixtureOutcome(input: {
  readonly fixture: FixtureMode;
  readonly stages: readonly string[];
  readonly failure: string | null;
  readonly disposed: boolean;
}): FixtureDisposition {
  if (input.fixture === "non-hibernating") {
    const requiredStages = [
      "ready",
      "original-socket-welcome",
      "owning-store-first-frame",
      "eviction-requested",
    ];
    const intendedRed =
      input.disposed &&
      input.failure ===
        "Timed out waiting to evict Durable Object: it still has active references." &&
      input.stages.length === requiredStages.length &&
      input.stages.every((stage, index): boolean => stage === requiredStages[index]);
    return intendedRed
      ? { kind: "expected-red", exitCode: 1 }
      : { kind: "invalid-control", exitCode: 2 };
  }
  return input.failure === null &&
    input.disposed &&
    input.stages.at(-1) === "same-socket-new-instance-restored-attachment-and-store"
    ? { kind: "passed", exitCode: 0 }
    : { kind: "failed", exitCode: 1 };
}

class WitnessRecorder {
  phase: z.infer<typeof witnessPhaseSchema> = "connecting";
  readonly constructors: ConstructorWitness[] = [];
  private handlers = 0;
  readonly counts = (): WitnessCounts => ({
    constructors: this.constructors.length,
    handlers: this.handlers,
  });
  readonly observe: V4FetchHandler = async (request): Promise<Response> => {
    const event = witnessEventSchema.parse(await request.json());
    if (event.kind === "constructor") {
      const instance = randomUUIDv7();
      this.constructors.push({ instance, phase: this.phase });
      return new Response(instance);
    }
    this.handlers += 1;
    return new Response("recorded");
  };
}

export function assertRestoredFrame(input: {
  readonly before: unknown;
  readonly after: unknown;
  readonly witness: string;
}): void {
  const before = observedSchema.parse(input.before);
  const after = observedSchema.parse(input.after);
  assert.notEqual(after.instance, before.instance, "real frame must run a new object instance");
  assert.deepEqual(
    after.attachment,
    { witness: input.witness },
    "original attachment must restore",
  );
  assert.equal(after.count, before.count + 1, "same owning SQLite state must survive eviction");
}

export interface HibernationFixtureReceipt {
  readonly fixture: FixtureMode;
  readonly attachmentWitness: string;
  readonly objectName: string;
  readonly workerSourceSha256: string;
  readonly stages: readonly string[];
  readonly constructors: number;
  readonly handlers: number;
  readonly welcome: z.infer<typeof welcomeSchema> | null;
  readonly before: z.infer<typeof observedSchema> | null;
  readonly after: z.infer<typeof observedSchema> | null;
  readonly heartbeatCounts: readonly WitnessCounts[];
  readonly constructorWitnesses: readonly ConstructorWitness[];
  readonly postSettlementCounts: WitnessCounts;
  readonly disposition: FixtureDisposition;
  readonly disposed: boolean;
  readonly failure: string | null;
  readonly productCoverage: "unobserved-C2-target-absent";
}

interface FixtureRunOptions {
  readonly fixture: FixtureMode;
  readonly createRuntime?: (construct: () => Miniflare) => Promise<Miniflare>;
}

function constructFixtureRuntime(input: {
  readonly fixture: FixtureMode;
  readonly objectName: string;
  readonly observe: V4FetchHandler;
}): Miniflare {
  return new Miniflare(
    convertV4MiniflareOptions({
      host: "127.0.0.1",
      port: 0,
      cf: false,
      workers: [
        {
          name: "hibernation-witness",
          compatibilityDate: "2026-08-21",
          modules: true,
          script: hibernationWorkerSource,
          durableObjects: { TARGET: { className: "HibernationWitness", useSQLite: true } },
          bindings: {
            HIBERNATING: String(input.fixture === "hibernating"),
            OBJECT_NAME: input.objectName,
          },
          serviceBindings: { WITNESS: input.observe },
          outboundService: (): never => {
            throw new Error("hibernation fixture forbids outbound fetch");
          },
        },
      ],
    }),
  );
}

// Only I/O completion and explicit events drive the journey: no elapsed time,
// wall-clock reads, sleeps, server timers or synthetic time assertions.
export async function runHibernationFixture(
  options: FixtureRunOptions,
): Promise<HibernationFixtureReceipt> {
  const attachmentWitness = randomUUIDv7();
  const objectName = randomUUIDv7();
  const stages: string[] = [];
  const recorder = new WitnessRecorder();
  let observationCounts = recorder.counts();
  let constructorWitnesses: readonly ConstructorWitness[] = [];
  let disposed = false;
  let failure: string | null = null;
  let runtime: Miniflare | undefined;
  let socket: Awaited<ReturnType<Miniflare["dispatchFetch"]>>["webSocket"] | undefined;
  let welcome: z.infer<typeof welcomeSchema> | null = null;
  let first: z.infer<typeof observedSchema> | null = null;
  let after: z.infer<typeof observedSchema> | null = null;
  const heartbeatCounts: WitnessCounts[] = [];
  try {
    const require = createRequire(import.meta.url);
    z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
    z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
    const construct = (): Miniflare => {
      const created = constructFixtureRuntime({
        fixture: options.fixture,
        objectName,
        observe: recorder.observe,
      });
      runtime = created; // Keep ownership even if the outer signal wrapper rejects.
      const dispose = created.dispose.bind(created);
      created.dispose = async (): Promise<void> => {
        await dispose();
        disposed = true;
      };
      return created;
    };
    runtime =
      options.createRuntime === undefined ? construct() : await options.createRuntime(construct);
    const ready = await runtime.ready;
    assert.equal(ready.hostname, "127.0.0.1");
    stages.push("ready");
    const response = await runtime.dispatchFetch(`${ready.origin}/socket`, {
      headers: { upgrade: "websocket", [attachmentWitnessHeader]: attachmentWitness },
    });
    assert.equal(response.status, 101);
    socket = response.webSocket;
    assert.ok(socket, "upgrade must return original socket");

    // Install before accept so the Welcome cannot be lost to listener ordering.
    const welcomeMessage = receiveFrame(socket);
    socket.accept();
    welcome = welcomeSchema.parse(JSON.parse(await welcomeMessage));
    assert.deepEqual(welcome.attachment, { witness: attachmentWitness });
    stages.push("original-socket-welcome");
    let welcomes = 1;
    socket.addEventListener("message", (event): void => {
      if (typeof event.data === "string" && event.data.includes('"kind":"welcome"')) welcomes += 1;
    });

    recorder.phase = "baseline";
    first = observedSchema.parse(JSON.parse(await exchangeFrame(socket, "observe")));
    assert.equal(first.instance, welcome.instance);
    assert.equal(first.count, 1);
    stages.push("owning-store-first-frame");
    after = await observeSocketAcrossHibernation({
      runtime,
      socket,
      objectName,
      attachmentWitness,
      first,
      recorder,
      stages,
      heartbeatCounts,
    });
    assertSingleWelcome(welcomes);
    stages.push("same-socket-new-instance-restored-attachment-and-store");
  } catch (error: unknown) {
    failure = error instanceof Error ? error.message : String(error);
  } finally {
    observationCounts = recorder.counts();
    constructorWitnesses = recorder.constructors.slice();
    recorder.phase = "settling";
    failure = await settleFixture({ runtime, socket, failure });
  }
  return {
    fixture: options.fixture,
    attachmentWitness,
    objectName,
    workerSourceSha256: createHash("sha256").update(hibernationWorkerSource).digest("hex"),
    stages,
    ...observationCounts,
    welcome,
    before: first,
    after,
    heartbeatCounts,
    constructorWitnesses,
    postSettlementCounts: recorder.counts(),
    disposition: classifyFixtureOutcome({ fixture: options.fixture, stages, failure, disposed }),
    disposed,
    failure,
    productCoverage: "unobserved-C2-target-absent",
  };
}

type LocalWebSocket = NonNullable<Awaited<ReturnType<Miniflare["dispatchFetch"]>>["webSocket"]>;

async function settleFixture(input: {
  readonly runtime: Miniflare | undefined;
  readonly socket: LocalWebSocket | null | undefined;
  readonly failure: string | null;
}): Promise<string | null> {
  let failure = input.failure;
  try {
    input.socket?.close(1000, "journey complete");
  } catch (error: unknown) {
    failure = `${failure ?? "journey passed"}; socket close: ${String(error)}`;
  }
  try {
    await input.runtime?.dispose();
  } catch (error: unknown) {
    failure = `${failure ?? "journey passed"}; cleanup: ${String(error)}`;
  }
  return failure;
}

async function observeSocketAcrossHibernation(input: {
  readonly runtime: Miniflare;
  readonly socket: LocalWebSocket;
  readonly objectName: string;
  readonly attachmentWitness: string;
  readonly first: z.infer<typeof observedSchema>;
  readonly recorder: WitnessRecorder;
  readonly stages: string[];
  readonly heartbeatCounts: WitnessCounts[];
}): Promise<z.infer<typeof observedSchema>> {
  const before = input.recorder.counts();
  input.recorder.phase = "evicting";
  input.stages.push("eviction-requested");
  await input.runtime.unsafeEvictDurableObject("hibernation-witness", "HibernationWitness", {
    name: input.objectName,
    webSockets: "hibernate",
  });
  input.stages.push("target-evicted");
  assertEvictionDidNotReconstruct(before, input.recorder.counts());
  // No target HTTP/inspection call between eviction and these original-socket frames.
  input.recorder.phase = "heartbeat";
  for (let index = 0; index < 3; index += 1) {
    assert.equal(await exchangeFrame(input.socket, "ping"), "pong");
    input.heartbeatCounts.push(input.recorder.counts());
    assertHeartbeatDidNotWake(before, input.recorder.counts());
  }
  input.stages.push("heartbeat-no-observed-wakes");
  input.recorder.phase = "deciding";
  const after = observedSchema.parse(JSON.parse(await exchangeFrame(input.socket, "observe")));
  assertRestoredFrame({ before: input.first, after, witness: input.attachmentWitness });
  assertSingleDecidingFrame(before, input.recorder.counts());
  assertDecidingConstructor(input.recorder.constructors, after.instance);
  return after;
}

function receiveFrame(socket: LocalWebSocket): Promise<string> {
  return new Promise((resolveFrame, rejectFrame): void => {
    const cleanup = (): void => {
      socket.removeEventListener("message", message);
      socket.removeEventListener("close", close);
      socket.removeEventListener("error", error);
    };
    const message = (event: MessageEvent): void => {
      cleanup();
      if (typeof event.data !== "string") rejectFrame(new Error("expected text frame"));
      else resolveFrame(event.data);
    };
    const close = (): void => {
      cleanup();
      rejectFrame(new Error("original socket closed before deciding frame"));
    };
    const error = (): void => {
      cleanup();
      rejectFrame(new Error("original socket error before deciding frame"));
    };
    socket.addEventListener("message", message);
    socket.addEventListener("close", close);
    socket.addEventListener("error", error);
  });
}

async function exchangeFrame(socket: LocalWebSocket, message: string): Promise<string> {
  const result = receiveFrame(socket);
  socket.send(message);
  return result;
}

async function main(arguments_: readonly string[]): Promise<void> {
  if (arguments_.length === 2 && arguments_[0] === "--c2-batch-plan") {
    await runC2ProductPlan(z.string().min(1).parse(arguments_[1]), "batch");
    return;
  }
  if (arguments_.length === 2 && arguments_[0] === "--c2-plan") {
    await runC2ProductPlan(z.string().min(1).parse(arguments_[1]));
    return;
  }
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    process.stdout.write(
      "Usage: node verification/observations/central/durable-object-hibernation.ts --fixture <non-hibernating|hibernating> --evidence <fresh-directory>\n" +
        "Or: --c2-plan <declared-frozen-target.json> (prebuilt signed Rust fixture, exact bundle hashes; no build/signing).\n" +
        "Or: --c2-batch-plan <declared-frozen-target.json> (same hold, exact batch fixture, one wake/independent Replies).\n" +
        "C2 plan: repository, sourceHead, evidence, binary{path,sha256}, publicBundle/privateBundle{indexJsSha256,wasmSha256}.\n" +
        "Run non-hibernating FIRST (expected-red exit 1; invalid control exit 2), then hibernating (exit 0).\n" +
        "Real local workerd stand-in; forced eviction, original socket, attachment/new instance, heartbeat witness phases.\n" +
        "Delayed constructor-only wake causality remains unverified.\n" +
        "Fixture mode gives no product C2/authentication proof. C2 mode remains draft until a real declared target is driven.\n" +
        "No natural idle, deployed billing or provider proof. No sleeps or SIGKILL.\n",
    );
    return;
  }
  assert.equal(arguments_.length, 4, "use --help");
  assert.equal(arguments_[0], "--fixture");
  assert.equal(arguments_[2], "--evidence");
  const fixture = fixtureModeSchema.parse(arguments_[1]);
  const evidence = resolve(z.string().min(1).parse(arguments_[3]));
  await mkdir(evidence); // A fresh directory prevents overwriting an earlier red.
  const receipt = await runHibernationFixture({
    fixture,
    createRuntime: constructMiniflareWithOwnedSignalShutdown,
  });
  const execution = {
    cwd: process.cwd(),
    sourceHead: execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim(),
    dirtyTree: execFileSync("git", ["status", "--porcelain"], { encoding: "utf8" }).trim() !== "",
    node: process.versions.node,
    libuv: process.versions.uv,
    miniflare: "5.20261012.0-beta",
    workerd: "1.20261012.1",
    driverSha256: createHash("sha256")
      .update(await readFile(new URL(import.meta.url)))
      .digest("hex"),
    signalShutdownSha256: createHash("sha256")
      .update(
        await readFile(new URL("../../test-support/miniflare-signal-shutdown.ts", import.meta.url)),
      )
      .digest("hex"),
  };
  const recorded = { ...receipt, execution };
  await writeFile(`${evidence}/receipt.json`, `${JSON.stringify(recorded, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(recorded)}\n`);
  process.exitCode = receipt.disposition.exitCode;
}

if (
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
)
  await main(process.argv.slice(2)).catch((error: unknown): void => {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 2;
  });
