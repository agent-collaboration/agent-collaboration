// The controlled peer's release gate, used by one held prompt at a time. behavior-agent.ts
// releases a held turn when its sentinel file appears and refuses a sentinel that already
// exists when the hold begins. One plan names one sentinel, so after a release the driver must
// remove its own marker before any later hold, or that hold would be released at once.
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";

const MARKER_PREFIX = "mcp-control release ";

/** Releases the held work, naming the operation it releases; never overwrites a marker. */
export function releaseHeldWork(sentinel: string, operationId: string): void {
  writeFileSync(sentinel, `${MARKER_PREFIX}${operationId}\n`, { flag: "wx" });
}

/** Removes the marker this run wrote for `operationId`; a marker it did not write stays put. */
export function retireReleaseMarker(sentinel: string, operationId: string): void {
  if (!existsSync(sentinel))
    throw new Error("the release marker is already gone; it was not this run's to remove");
  if (readFileSync(sentinel, "utf8") !== `${MARKER_PREFIX}${operationId}\n`)
    throw new Error("the release sentinel holds a marker this run did not write");
  rmSync(sentinel);
}

/** A later hold may start only when no release marker remains. */
export function requireNoReleaseMarker(sentinel: string): void {
  if (existsSync(sentinel))
    throw new Error("a release marker remains; a new hold would be released at once");
}
