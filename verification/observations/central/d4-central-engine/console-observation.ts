/** Retain local workerd's original structured console messages and check S1 order. */
import type { WorkerdStructuredLog } from "miniflare";
import { encode } from "../../../exact-json.ts";
import {
  D4EventSequence,
  type D4ConstructionRecord,
  type D4EventRecord,
} from "./event-observation.ts";

export class D4WorkerdConsole {
  private readonly sequence = new D4EventSequence();
  private readonly lines: string[] = [];
  private bytes = 0;
  private failure: unknown;

  observe(record: WorkerdStructuredLog): void {
    if (this.failure !== undefined) return;
    try {
      const line = `${encode(record)}\n`;
      if (this.bytes + Buffer.byteLength(line) > 32 * 1024 * 1024)
        throw new Error("D4 workerd console exceeds 32 MiB");
      this.lines.push(line);
      this.bytes += Buffer.byteLength(line);
      this.sequence.observeLine(record.message);
    } catch (error: unknown) {
      // Observation must not change product execution or lose a failed decoder
      // behind an otherwise successful Frame. Every read checks this latch.
      this.failure = error;
    }
  }
  private requireHealthy(): void {
    if (this.failure !== undefined)
      throw new Error("D4 workerd console observation failed", { cause: this.failure });
  }
  records(): readonly D4EventRecord[] {
    this.requireHealthy();
    return this.sequence.records();
  }
  constructionRecords(): readonly D4ConstructionRecord[] {
    this.requireHealthy();
    return this.sequence.constructionRecords();
  }
  transcript(): string {
    this.requireHealthy();
    this.sequence.requireComplete();
    return this.lines.join("");
  }
}
