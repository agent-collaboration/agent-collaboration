/** Observe real forwarded server payloads; never synthesize a product response. */
import { z } from "zod";
import { encode } from "../../../exact-json.ts";
import type { SessionSnapshot } from "../../../access/session-audit.ts";

const frameSchema = z.object({
  kind: z.enum(["welcome", "answer", "request", "notice", "closed"]),
  value: z.record(z.string(), z.unknown()),
});
const pageSchema = z.object({
  kind: z.literal("page"),
  value: z.object({ notices: z.array(z.object({ position: z.bigint().nonnegative() })) }),
});

export interface CompatibilityWireSnapshot {
  readonly frames: number;
  readonly welcomes: number;
  readonly noticePositions: readonly string[];
  readonly operationKinds: readonly string[];
  readonly refusalFrames: readonly string[];
  readonly unsupportedFrames: number;
}

export class CompatibilityWireObservation {
  private frames = 0;
  private welcomes = 0;
  private unsupportedFrames = 0;
  private readonly positions = new Set<string>();
  private readonly operationKinds = new Set<string>();
  private readonly refusals: string[] = [];
  private readonly rawFrames: unknown[] = [];

  observe(value: unknown): void {
    if (this.rawFrames.length >= 16384)
      throw new Error("compatibility frame evidence bound exceeded");
    this.rawFrames.push(value);
    this.frames += 1;
    const parsed = frameSchema.safeParse(value);
    if (!parsed.success) {
      this.unsupportedFrames += 1;
      return;
    }
    const frame = parsed.data;
    if (frame.kind === "welcome") {
      this.welcomes += 1;
      if (Object.hasOwn(frame.value, "capabilities")) this.unsupportedFrames += 1;
    }
    this.walk(frame.value, frame.kind);
  }
  private walk(value: unknown, path: string): void {
    const object = z.record(z.string(), z.unknown()).safeParse(value);
    if (!object.success) return;
    const kind = typeof object.data.kind === "string" ? object.data.kind : undefined;
    if (kind !== undefined) {
      this.operationKinds.add(`${path}/${kind}`);
      if (
        ["refused", "invalidShape", "outcomeUnknown", "deliveryUnavailable", "closed"].includes(
          kind,
        )
      )
        this.refusals.push(encode(value));
    }
    const page = pageSchema.safeParse(object.data);
    if (page.success)
      for (const notice of page.data.value.notices) this.positions.add(notice.position.toString());
    // NoticeRead is internally tagged; all other envelopes use value/body.
    for (const key of ["value", "body"]) {
      const nested = object.data[key];
      if (nested !== undefined) this.walk(nested, `${path}/${kind ?? key}`);
    }
  }
  snapshot(): CompatibilityWireSnapshot {
    return {
      frames: this.frames,
      welcomes: this.welcomes,
      noticePositions: [...this.positions],
      operationKinds: [...this.operationKinds],
      refusalFrames: [...this.refusals],
      unsupportedFrames: this.unsupportedFrames,
    };
  }
  observations(): readonly unknown[] {
    return [...this.rawFrames];
  }
}

export function requireCompatibleWire(snapshot: CompatibilityWireSnapshot): void {
  if (
    snapshot.welcomes === 0 ||
    snapshot.unsupportedFrames !== 0 ||
    snapshot.refusalFrames.length !== 0
  )
    throw new Error("legacy wire refused, changed, or never welcomed");
}

export interface CompatibilityEndpointEvidence {
  readonly wire: CompatibilityWireSnapshot;
  readonly audit: SessionSnapshot | undefined;
}

export function requireCompatiblePair(endpoints: {
  readonly a: CompatibilityEndpointEvidence;
  readonly b: CompatibilityEndpointEvidence;
}): void {
  for (const [name, endpoint] of Object.entries(endpoints)) {
    if (
      endpoint.audit === undefined ||
      endpoint.audit.invalidConnections !== 0 ||
      endpoint.audit.welcomes === 0
    )
      throw new Error(`incomplete or invalid ${name} raw Session audit`);
    requireCompatibleWire(endpoint.wire);
  }
}

export function requireExactBacklog(expected: readonly string[], pushed: readonly string[]): void {
  const selected = pushed.filter((id): boolean => expected.includes(id));
  if (
    expected.length !== 50 ||
    new Set(expected).size !== 50 ||
    selected.length !== 50 ||
    expected.some((id): boolean => selected.filter((actual): boolean => actual === id).length !== 1)
  )
    throw new Error("50 exact backlog items must arrive once as single pushes");
}
