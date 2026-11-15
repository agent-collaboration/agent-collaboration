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
import { hibernationWorkerSource } from "../../test-support/hibernation-worker.ts";
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

interface WitnessCounts {
  readonly constructors: number;
  readonly handlers: number;
}

export function assertHeartbeatDidNotWake(before: WitnessCounts, after: WitnessCounts): void {
  assert.deepEqual(after, before, "heartbeat must run neither constructor nor handler");
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
  readonly attachmentWitness: string;
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
            ATTACHMENT_WITNESS: input.attachmentWitness,
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
  const require = createRequire(import.meta.url);
  z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
  z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
  const attachmentWitness = randomUUIDv7();
  const objectName = randomUUIDv7();
  const stages: string[] = [];
  let constructors = 0;
  let handlers = 0;
  let disposed = false;
  let failure: string | null = null;
  let runtime: Miniflare | undefined;
  let socket: Awaited<ReturnType<Miniflare["dispatchFetch"]>>["webSocket"] | undefined;
  let welcome: z.infer<typeof welcomeSchema> | null = null;
  let first: z.infer<typeof observedSchema> | null = null;
  let after: z.infer<typeof observedSchema> | null = null;
  const heartbeatCounts: WitnessCounts[] = [];
  const counts = (): WitnessCounts => ({ constructors, handlers });
  try {
    const construct = (): Miniflare =>
      constructFixtureRuntime({
        fixture: options.fixture,
        attachmentWitness,
        objectName,
        observe: async (request: Parameters<V4FetchHandler>[0]): Promise<Response> => {
          const event = witnessEventSchema.parse(await request.json());
          if (event.kind === "constructor") {
            constructors += 1;
            return new Response(randomUUIDv7());
          }
          handlers += 1;
          return new Response("recorded");
        },
      });
    runtime =
      options.createRuntime === undefined ? construct() : await options.createRuntime(construct);
    const ready = await runtime.ready;
    assert.equal(ready.hostname, "127.0.0.1");
    stages.push("ready");
    const response = await runtime.dispatchFetch(`${ready.origin}/socket`, {
      headers: { upgrade: "websocket" },
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
      counts,
      stages,
      heartbeatCounts,
    });
    assert.equal(welcomes, 1, "no reconnect or replacement Welcome");
    stages.push("same-socket-new-instance-restored-attachment-and-store");
  } catch (error: unknown) {
    failure = error instanceof Error ? error.message : String(error);
  } finally {
    const settlement = await settleFixture({ runtime, socket, failure });
    failure = settlement.failure;
    disposed = settlement.disposed;
  }
  return {
    fixture: options.fixture,
    attachmentWitness,
    objectName,
    workerSourceSha256: createHash("sha256").update(hibernationWorkerSource).digest("hex"),
    stages,
    constructors,
    handlers,
    welcome,
    before: first,
    after,
    heartbeatCounts,
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
}): Promise<{ readonly failure: string | null; readonly disposed: boolean }> {
  let failure = input.failure;
  let disposed = false;
  try {
    input.socket?.close(1000, "journey complete");
  } catch (error: unknown) {
    failure = `${failure ?? "journey passed"}; socket close: ${String(error)}`;
  }
  try {
    await input.runtime?.dispose();
    disposed = input.runtime !== undefined;
  } catch (error: unknown) {
    failure = `${failure ?? "journey passed"}; cleanup: ${String(error)}`;
  }
  return { failure, disposed };
}

async function observeSocketAcrossHibernation(input: {
  readonly runtime: Miniflare;
  readonly socket: LocalWebSocket;
  readonly objectName: string;
  readonly attachmentWitness: string;
  readonly first: z.infer<typeof observedSchema>;
  readonly counts: () => WitnessCounts;
  readonly stages: string[];
  readonly heartbeatCounts: WitnessCounts[];
}): Promise<z.infer<typeof observedSchema>> {
  const before = input.counts();
  input.stages.push("eviction-requested");
  await input.runtime.unsafeEvictDurableObject("hibernation-witness", "HibernationWitness", {
    name: input.objectName,
    webSockets: "hibernate",
  });
  input.stages.push("target-evicted");
  assert.deepEqual(input.counts(), before, "eviction itself must not reconstruct the object");
  // No target HTTP/inspection call between eviction and these original-socket frames.
  for (let index = 0; index < 3; index += 1) {
    assert.equal(await exchangeFrame(input.socket, "ping"), "pong");
    input.heartbeatCounts.push(input.counts());
    assertHeartbeatDidNotWake(before, input.counts());
  }
  input.stages.push("heartbeat-zero-wakes");
  const after = observedSchema.parse(JSON.parse(await exchangeFrame(input.socket, "observe")));
  assertRestoredFrame({ before: input.first, after, witness: input.attachmentWitness });
  assert.deepEqual(input.counts(), {
    constructors: before.constructors + 1,
    handlers: before.handlers + 1,
  });
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
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    process.stdout.write(
      "Usage: node verification/observations/central/durable-object-hibernation.ts --fixture <non-hibernating|hibernating> --evidence <fresh-directory>\n" +
        "Run non-hibernating FIRST (required exit 1), then hibernating (exit 0).\n" +
        "Real local workerd stand-in; targeted forced eviction, same socket, attachment/new instance, heartbeat zero wakes.\n" +
        "No product C2, natural idle, deployed billing, authentication or provider proof. No sleeps or SIGKILL.\n",
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
    node: process.versions.node,
    libuv: process.versions.uv,
    miniflare: "5.20261012.0-beta",
    workerd: "1.20261012.1",
    driverSha256: createHash("sha256")
      .update(await readFile(new URL(import.meta.url)))
      .digest("hex"),
  };
  const recorded = { ...receipt, execution };
  await writeFile(`${evidence}/receipt.json`, `${JSON.stringify(recorded, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(recorded)}\n`);
  if (receipt.failure !== null || !receipt.disposed) process.exitCode = 1;
}

if (
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
)
  await main(process.argv.slice(2));
