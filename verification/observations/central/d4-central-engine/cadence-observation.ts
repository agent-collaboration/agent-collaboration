/** J5 resident-case observations from complete, caller-bound S1 intervals. */
import {
  d4EventRecordSchema,
  epochMillisecondsSchema,
  type D4EventRecord,
} from "./event-observation.ts";
type ClaimedBatch = Extract<NonNullable<D4EventRecord["batch"]>, { claimed: true }>;
type DueSourceName = ClaimedBatch["sources"][number]["source"];

export class CadenceFailure extends Error {
  readonly check: string;
  readonly actual: unknown;
  constructor(check: string, actual: unknown) {
    super(`J5 assertion failed: ${check}`);
    this.check = check;
    this.actual = actual;
  }
}
function claimedBatches(records: readonly D4EventRecord[]) {
  const parsed = records.map((record): D4EventRecord => d4EventRecordSchema.parse(record));
  if (
    parsed.length === 0 ||
    new Set(parsed.map((record): string => record.eventId)).size !== parsed.length
  )
    throw new CadenceFailure("complete-source-interval", "no events or repeated event UUID");
  if (
    parsed.some((record): boolean => record.object.kind !== "fleet") ||
    new Set(parsed.map((record): string => record.construction)).size !== 1
  )
    throw new CadenceFailure(
      "resident-no-second-batch-within-30s",
      "expected one resident Fleet construction",
    );
  return parsed
    .flatMap((record) =>
      record.batch?.claimed === true ? [{ eventId: record.eventId, ...record.batch }] : [],
    )
    .sort((left, right): number =>
      left.startedAt < right.startedAt ? -1 : left.startedAt > right.startedAt ? 1 : 0,
    );
}

export function observeResidentCadence(input: {
  readonly records: readonly D4EventRecord[];
  readonly workload: "100-one-batch" | "1200-three-batches";
  readonly source: DueSourceName;
  readonly dueAt: bigint;
}) {
  const dueAt = epochMillisecondsSchema.parse(input.dueAt);
  const expectedItems = input.workload === "100-one-batch" ? 100n : 1200n;
  const expectedBatches = input.workload === "1200-three-batches" ? 3 : 1;
  const batches = claimedBatches(input.records);
  for (const [index, batch] of batches.entries()) {
    if (batch.startedAt < dueAt || batch.endedAt < batch.startedAt)
      throw new CadenceFailure("actual-due-batch-timing", batch);
    const previous = batches[index - 1];
    if (previous !== undefined && batch.startedAt - previous.startedAt < 30_000n)
      throw new CadenceFailure("resident-no-second-batch-within-30s", { previous, batch });
  }
  if (batches.length !== expectedBatches)
    throw new CadenceFailure(input.workload, { expectedBatches, batches });
  let done = 0n;
  for (const batch of batches) {
    const sources = batch.sources.filter((source): boolean => source.source === input.source);
    if (sources.length !== 1)
      throw new CadenceFailure("actual-item-counts-required", batch.sources);
    const source = sources[0];
    if (source === undefined) throw new Error("missing source");
    if (source.done > 500n || source.failed !== 0n || source.done === 0n)
      throw new CadenceFailure("500-items-per-source", source);
    done += source.done;
    if (done > expectedItems || source.remaining !== expectedItems - done)
      throw new CadenceFailure(input.workload, {
        done,
        remaining: source.remaining,
        expectedItems,
      });
  }
  if (done !== expectedItems) throw new CadenceFailure(input.workload, { done, expectedItems });
  return {
    workload: input.workload,
    source: input.source,
    done,
    batches,
    runtimeQualified: false as const,
    remainingQualification: [
      "complete original source/bundle-bound S1 interval and actual fixture effects",
      "isolated wrong-product cadence/cap sensitivity and restored positive",
      "singleton effect-by-T+30s, duplicate-alarm and one-reconstruction allowances are separate cases",
    ],
  };
}
