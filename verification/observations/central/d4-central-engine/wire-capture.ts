/** Bounded original-byte transcript for an explicitly allocated Session gate. */
import type { SessionByteRecord } from "../../../access/session-gate.ts";
import { encode } from "../../../exact-json.ts";
import { windowTraceRecordSchema } from "./window-readback.ts";

export class D4WireCapture {
  private readonly lines: string[] = [];
  private bytes = 0;
  private failure: unknown;
  observe(record: SessionByteRecord): void {
    this.requireHealthy();
    try {
      const line = `${encode(windowTraceRecordSchema.parse(record))}\n`;
      if (this.bytes + Buffer.byteLength(line) > 32 * 1024 * 1024)
        throw new Error("D4 original-wire capture exceeds 32 MiB");
      this.lines.push(line);
      this.bytes += Buffer.byteLength(line);
    } catch (error: unknown) {
      this.failure = error instanceof Error ? error : new Error(String(error));
      throw this.failure;
    }
  }
  requireHealthy(): void {
    if (this.failure !== undefined)
      throw new Error("D4 original-wire capture invalid", { cause: this.failure });
  }
  transcript(): string {
    this.requireHealthy();
    if (this.lines.length === 0) throw new Error("no original Session bytes observed");
    return this.lines.join("");
  }
}
