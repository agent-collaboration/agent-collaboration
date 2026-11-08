/** Drive two actual Tokio Alarm events into distinct one-shot beforeClaim latches. */
import { D4ControlClient } from "./control-client.ts";
import type { D4EventRecord } from "./event-observation.ts";

export class ClaimOverlapFailure extends Error {
  readonly check: "overlapping-claim" | "no-duplicate-batch";
  readonly actual: unknown;
  constructor(check: "overlapping-claim" | "no-duplicate-batch", actual: unknown) {
    super(`J18 assertion failed: ${check}`);
    this.check = check;
    this.actual = actual;
  }
}
function requireClaimPair(events: readonly D4EventRecord[], held: readonly string[]): void {
  if (
    events.length !== 2 ||
    new Set(events.map((event): string => event.eventId)).size !== 2 ||
    held.some((id): boolean => !events.some((event): boolean => event.eventId === id)) ||
    events.some((event): boolean => event.event.kind !== "alarm" || event.batch === null) ||
    new Set(events.map((event): string => event.construction)).size !== 1
  )
    throw new ClaimOverlapFailure("overlapping-claim", { events, held });
  if (events.filter((event): boolean => event.batch?.claimed === true).length !== 1)
    throw new ClaimOverlapFailure("no-duplicate-batch", events);
}

export async function driveTokioClaimOverlap(controls: D4ControlClient) {
  const pending = await controls.readPending();
  if (Object.values(pending).some((count): boolean => count !== 0n))
    throw new ClaimOverlapFailure("overlapping-claim", { pending });
  const baseline = await controls.readEvents(0n);
  const barriers: string[] = [];
  let held: string[] = [];
  let completion: Promise<PromiseSettledResult<void>[]> | undefined;
  let failure: { error: unknown } | undefined;
  const cleanupFailures: unknown[] = [];
  try {
    barriers.push(await controls.barrier("beforeClaim", null));
    barriers.push(await controls.barrier("beforeClaim", null));
    if (barriers[0] === barriers[1]) throw new Error("same latch returned twice");
    // Start both real actions, attach rejection handlers now, and keep them
    // pending while both distinct entrant identities reach their latches.
    completion = Promise.allSettled([controls.duplicateAlarm(), controls.duplicateAlarm()]);
    const reached = await Promise.allSettled(
      barriers.map((id): Promise<string> => controls.awaitBarrier(id)),
    );
    for (const result of reached) {
      if (result.status === "rejected") throw result.reason;
      held.push(result.value);
    }
    if (new Set(held).size !== 2) throw new ClaimOverlapFailure("overlapping-claim", { held });
  } catch (error: unknown) {
    failure = { error };
  } finally {
    // Release each owned latch exactly once even on observation failure. An
    // uncertain mutation is never retried; cleanup failure prevents credit.
    const releases = await Promise.allSettled(
      [...new Set(barriers)].map((id): Promise<void> => controls.releaseBarrier(id)),
    );
    for (const result of releases)
      if (result.status === "rejected") cleanupFailures.push(result.reason);
    if (completion !== undefined)
      for (const result of await completion)
        if (result.status === "rejected") cleanupFailures.push(result.reason);
  }
  if (cleanupFailures.length > 0)
    throw new AggregateError(
      [...(failure === undefined ? [] : [failure.error]), ...cleanupFailures],
      "J18 actions or owned latch cleanup did not settle",
    );
  if (failure !== undefined) throw failure.error;
  const delta = await controls.readEvents(baseline.through);
  const events = delta.records.map((item): D4EventRecord => item.record);
  requireClaimPair(events, held);
  const after = await controls.readPending();
  if (Object.values(after).some((count): boolean => count !== 0n))
    throw new ClaimOverlapFailure("overlapping-claim", { after });
  return {
    journey: "J18" as const,
    shell: "tokio" as const,
    barriers,
    heldEvents: held,
    records: events,
    runtimeQualified: false as const,
    remainingQualification: [
      "declared target, complete producer and source-bound one-shot event latch qualification",
      "fixture has due work, no earlier cadence claim, no automatic Alarm or unrelated input during overlap",
      "actual workload effect and isolated wrong-product atomic-claim sensitivity with restored positive",
      "earlier-commit and step-lower recompute interleavings are separate J18 cases",
    ],
  };
}
