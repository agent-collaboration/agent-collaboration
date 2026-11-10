/** Decode the declared worker_loop stderr events; absence of a worker never passes. */
import { stripVTControlCharacters } from "node:util";
import { z } from "zod";

const workerEventSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("pass-started"), worker: z.string().min(1), line: z.string() }),
  z.object({
    kind: z.literal("sleeping"),
    worker: z.string().min(1),
    deadline: z.bigint().nullable(),
    line: z.string(),
  }),
]);
export type WorkerLoopEvent = z.infer<typeof workerEventSchema>;

export function readWorkerEvents(stderr: string): WorkerLoopEvent[] {
  const events: WorkerLoopEvent[] = [];
  for (const original of stderr.split("\n")) {
    const line = stripVTControlCharacters(original);
    if (!line.includes("worker_loop:")) continue;
    const phase = /worker_loop:\s+(pass started|sleeping)\b/.exec(line)?.[1];
    const workerMatch = /\bworker=(?:"([^"]+)"|(\S+))/.exec(line);
    const worker = workerMatch?.[1] ?? workerMatch?.[2];
    if (phase === undefined || worker === undefined)
      throw new Error("unrecognized worker_loop event; keep raw stderr and qualify the source");
    if (phase === "pass started") {
      events.push(workerEventSchema.parse({ kind: "pass-started", worker, line }));
      continue;
    }
    const armed = /\barmed=(None|Some\((-?\d+)\))/.exec(line);
    if (armed === null) throw new Error("sleeping worker event lacks its armed deadline");
    const deadline = armed[1] === "None" ? null : BigInt(z.string().parse(armed[2]));
    events.push(workerEventSchema.parse({ kind: "sleeping", worker, deadline, line }));
  }
  return events;
}

export interface WorkerStartupState {
  readonly worker: string;
  readonly passes: number;
  readonly sleeps: number;
  readonly deadline: bigint | null;
}

export function inspectWorkerStartup(input: {
  readonly events: readonly WorkerLoopEvent[];
  readonly expectedWorkers: readonly string[];
}): readonly WorkerStartupState[] | null {
  if (
    input.expectedWorkers.length === 0 ||
    new Set(input.expectedWorkers).size !== input.expectedWorkers.length
  )
    throw new Error("declare a nonempty distinct worker inventory");
  const states: WorkerStartupState[] = [];
  for (const worker of input.expectedWorkers) {
    const events = input.events.filter((event) => event.worker === worker);
    const firstPass = events.findIndex((event) => event.kind === "pass-started");
    const last = events.at(-1);
    if (firstPass !== 0 || last?.kind !== "sleeping" || events.indexOf(last) <= firstPass)
      return null;
    states.push({
      worker,
      passes: events.filter((event) => event.kind === "pass-started").length,
      sleeps: events.filter((event) => event.kind === "sleeping").length,
      deadline: last.deadline,
    });
  }
  return states;
}

export function requireIdleWorkerLog(input: {
  readonly before: readonly WorkerLoopEvent[];
  readonly after: readonly WorkerLoopEvent[];
}): void {
  if (
    input.after.length !== input.before.length ||
    input.before.some((event, index) => event.line !== input.after[index]?.line)
  )
    throw new Error("worker_loop events changed during the declared idle window");
}
