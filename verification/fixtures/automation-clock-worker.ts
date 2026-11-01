/** Local debug harness: virtualize only Fleet's three alarm capabilities. */
import {
  FleetObject as RustFleetObject,
  type WorkerState,
  type WorkerStorage,
} from "central-worker-under-test";
import { z } from "zod";
export {
  default,
  BootstrapObject,
  ConnectionObject,
  LeaseRegistryObject,
  MailboxObject,
  ProjectCopyObject,
} from "central-worker-under-test";

const alarmKey = "_cf_automation_verification_alarm";
const clockKey = "_cf_composed_clock";
const receiptsKey = "_cf_automation_verification_receipts";
const failuresKey = "_cf_automation_verification_failures";
const maximumCallbacks = 10_000;
const timestampSchema = z.iso.datetime({ offset: true });
const millisecondsSchema = z.number().int().safe();
const receiptSchema = z.object({
  atMs: millisecondsSchema,
  result: z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("completed") }),
    z.object({ kind: z.literal("failed") }),
  ]),
  nextAlarmMs: millisecondsSchema.nullable(),
});
type AlarmReceipt = z.infer<typeof receiptSchema>;
const receiptsSchema = z.array(receiptSchema).max(maximumCallbacks);
const controlSchema = z.object({
  kind: z.literal("automationClock"),
  value: z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("start"), now: timestampSchema }),
    z.object({ kind: z.literal("advance"), now: timestampSchema }),
    z.object({ kind: z.literal("observe") }),
    z.object({ kind: z.literal("invokeAlarm") }),
    z.object({ kind: z.literal("removeAlarm") }),
  ]),
});
type ClockAction = z.infer<typeof controlSchema>["value"]["kind"];

function boundMember(target: object, property: string | symbol): unknown {
  const value: unknown = Reflect.get(target, property, target);
  return typeof value === "function" ? value.bind(target) : value;
}

async function readDeadline(storage: WorkerStorage): Promise<number | null> {
  const value: unknown = await storage.get(alarmKey);
  return value === undefined ? null : millisecondsSchema.parse(value);
}

function virtualAlarmStorage(storage: WorkerStorage): WorkerStorage {
  return new Proxy(storage, {
    get(target: WorkerStorage, property: string | symbol): unknown {
      if (property === "getAlarm") {
        return async (): Promise<number | null> => readDeadline(target);
      }
      if (property === "setAlarm") {
        return async (at: Date | number): Promise<void> => {
          const deadline = millisecondsSchema.parse(at instanceof Date ? at.getTime() : at);
          await target.put(alarmKey, deadline);
          await target.put(failuresKey, 0);
        };
      }
      if (property === "deleteAlarm") {
        return async (): Promise<void> => {
          await target.delete(alarmKey);
        };
      }
      return boundMember(target, property);
    },
  });
}

function virtualAlarmState(state: WorkerState): WorkerState {
  const storage = virtualAlarmStorage(state.storage);
  return new Proxy(state, {
    get(target: WorkerState, property: string | symbol): unknown {
      return property === "storage" ? storage : boundMember(target, property);
    },
  });
}

export class FleetObject extends RustFleetObject {
  private readonly controlledStorage: WorkerStorage;
  private clockTail: Promise<void> = Promise.resolve();

  constructor(state: WorkerState, environment: unknown) {
    super(virtualAlarmState(state), environment);
    this.controlledStorage = state.storage;
  }

  override async fetch(request: Request): Promise<Response> {
    if (new URL(request.url).pathname !== "/test/runtime-control") {
      return super.fetch(request);
    }
    const input: unknown = await request.clone().json();
    const parsed = controlSchema.safeParse(input);
    if (!parsed.success) return super.fetch(request);
    const previous = this.clockTail;
    const finished = Promise.withResolvers<void>();
    this.clockTail = finished.promise;
    await previous;
    try {
      return await this.controlledFetch(request, parsed.data.value.kind);
    } finally {
      finished.resolve();
    }
  }

  private async controlledFetch(request: Request, action: ClockAction): Promise<Response> {
    const response = await super.fetch(request);
    if (!response.ok) return response;
    if (response.headers.get("x-central-debug-automation-clock") !== "v1") {
      throw new Error("controlled alarms require the debug Rust clock build");
    }
    const now = timestampSchema.parse(await this.controlledStorage.get(clockKey));
    const nowMs = millisecondsSchema.parse(Date.parse(now));
    switch (action) {
      case "invokeAlarm":
        await this.controlledStorage.delete(alarmKey);
        await this.alarm();
        await this.drain(nowMs);
        break;
      case "advance":
        await this.drain(nowMs);
        break;
      case "removeAlarm":
        await this.controlledStorage.delete(alarmKey);
        break;
      case "start":
      case "observe":
        break;
    }
    const raw: unknown = await response.json();
    const observation = z.looseObject({ clock: z.unknown() }).parse(raw);
    const result = {
      ...observation,
      automationClock: {
        now,
        nextAlarmMs: await readDeadline(this.controlledStorage),
        callbacks: await this.receipts(),
      },
    };
    return Response.json(result);
  }

  override async alarm(): Promise<void> {
    const now = timestampSchema.parse(await this.controlledStorage.get(clockKey));
    const nowMs = millisecondsSchema.parse(Date.parse(now));
    const callbacks = await this.receipts();
    if (callbacks.length >= maximumCallbacks)
      throw new Error("verification callback limit reached");
    let result: AlarmReceipt["result"] = { kind: "completed" };
    try {
      // This is the generated Rust FleetObject.alarm -> ShellActor -> on_alarm.
      await super.alarm();
      await this.controlledStorage.put(failuresKey, 0);
    } catch {
      result = { kind: "failed" };
      const raw: unknown = await this.controlledStorage.get(failuresKey);
      const failed = raw === undefined ? 1 : z.number().int().min(0).max(6).parse(raw) + 1;
      await this.controlledStorage.put(failuresKey, failed);
      // Simulated retry/exhaustion policy, not native Cloudflare retry proof.
      if (failed < 6) {
        const retry = nowMs + 2000 * 2 ** (failed - 1);
        const existing = await readDeadline(this.controlledStorage);
        await this.controlledStorage.put(
          alarmKey,
          existing === null ? retry : Math.min(existing, retry),
        );
      } else {
        await this.controlledStorage.delete(alarmKey);
      }
    }
    callbacks.push({
      atMs: nowMs,
      result,
      nextAlarmMs: await readDeadline(this.controlledStorage),
    });
    await this.controlledStorage.put(receiptsKey, callbacks);
  }

  private async receipts(): Promise<AlarmReceipt[]> {
    const value: unknown = await this.controlledStorage.get(receiptsKey);
    return value === undefined ? [] : receiptsSchema.parse(value);
  }

  private async drain(nowMs: number): Promise<void> {
    for (let count = 0; count < maximumCallbacks; count += 1) {
      const deadline = await readDeadline(this.controlledStorage);
      if (deadline === null || deadline > nowMs) return;
      // A real platform consumes its alarm before invoking the handler.
      await this.controlledStorage.delete(alarmKey);
      await this.alarm();
    }
    throw new Error("verification advance ended with pending due callbacks");
  }
}
