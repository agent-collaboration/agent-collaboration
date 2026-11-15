// Continue provider hub pages using the existing observation ledger. Native observations
// have no epoch/cursor and are deliberately outside this provider-only path.
import {
  foldObservationReads,
  type ObservationRead,
  type ObservationLedger,
} from "../../mcp-control/observations/observation-ledger.ts";
import type { SessionRef } from "../../mcp-control/observations/observation-models.ts";
import { isDeepStrictEqual } from "node:util";

type ContinuousLedger = Extract<ObservationLedger, { readonly kind: "continuous" }>;
export interface PageCursor {
  readonly epoch: bigint | null;
  readonly afterSequence: bigint | null;
}
export interface ProviderPages {
  readonly reads: readonly ObservationRead[];
  readonly baselineReads: number;
  readonly ledger: ContinuousLedger;
}

function requireCompleteLedger(
  target: SessionRef,
  reads: readonly ObservationRead[],
  baselineReads: number,
): ContinuousLedger {
  const ledger = foldObservationReads(target, reads, baselineReads);
  if (ledger.kind !== "continuous") throw new Error(`provider observation ${ledger.kind}`);
  if (ledger.backendDisconnected) throw new Error("provider observation backend disconnected");
  if (ledger.baselineTooLarge.length > 0 || ledger.tooLarge.length > 0)
    throw new Error("provider observation contains an oversized-event gap");
  if (
    reads.some(
      (read): boolean => !read.result.attached || read.result.endReason === "callerCancelled",
    )
  )
    throw new Error("provider observation did not remain attached");
  if (
    reads.some(
      (read): boolean => !isDeepStrictEqual(read.result.generation, reads[0]?.result.generation),
    )
  )
    throw new Error("provider observation binding generation changed");
  return ledger;
}

function cursorAfter(ledger: ContinuousLedger): PageCursor {
  return {
    epoch: ledger.epoch,
    afterSequence: ledger.events.at(-1)?.sequence ?? ledger.baselineSequence,
  };
}

/** Drain a result-limited baseline or continuation before allowing the next driving step. */
export async function captureProviderPages(input: {
  readonly target: SessionRef;
  readonly previous: ProviderPages | null;
  readonly readPage: (cursor: PageCursor) => Promise<ObservationRead>;
  readonly recordPage: (read: ObservationRead, index: number) => void;
  readonly maxPages?: number;
}): Promise<ProviderPages> {
  const reads = [...(input.previous?.reads ?? [])];
  const baseline = input.previous === null;
  const maxPages = input.maxPages ?? 10;
  if (!Number.isSafeInteger(maxPages) || maxPages < 1)
    throw new Error("positive page bound required");
  let ledger =
    input.previous === null
      ? null
      : requireCompleteLedger(input.target, reads, input.previous.baselineReads);
  for (let page = 0; page < maxPages; page += 1) {
    const cursor = ledger === null ? { epoch: null, afterSequence: null } : cursorAfter(ledger);
    const read = await input.readPage(cursor);
    reads.push(read);
    // Preserve even a rejected page before evaluating it; no mutation is retried.
    input.recordPage(read, reads.length - 1);
    const baselineReads = baseline ? reads.length : (input.previous?.baselineReads ?? 1);
    ledger = requireCompleteLedger(input.target, reads, baselineReads);
    if (read.result.endReason === "deadlineReached") return { reads, baselineReads, ledger };
    if (read.result.endReason !== "resultLimitReached")
      throw new Error("provider observation ended incompletely");
    if (read.result.events.length === 0)
      throw new Error("provider observation page made no progress");
  }
  throw new Error("provider observation page bound exhausted; no further input");
}

/** Only coverage metadata leaves the in-memory ledger; model item bodies stay out of files. */
export function providerPageSummary(pages: ProviderPages): Readonly<Record<string, unknown>> {
  return {
    kind: "providerCursorChecked",
    baselineReads: pages.baselineReads,
    reads: pages.reads.length,
    epoch: pages.ledger.epoch,
    baselineSequence: pages.ledger.baselineSequence,
    finalSequence: cursorAfter(pages.ledger).afterSequence,
    postBaselineEvents: pages.ledger.events.length,
    // The service always sets this flag: it describes call-local attachment, not cursor loss.
    continuationGap: true,
    productVerdict: "notEvaluated",
  };
}
