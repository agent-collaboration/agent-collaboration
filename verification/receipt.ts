import { appendFile, mkdir, writeFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { createHash } from "node:crypto";
import { encode } from "./exact-json.ts";
import type { Declaration } from "./declaration.ts";

export class ObservationOpen extends Error {}
export class ObservationFailure extends Error {}
export type RowOutcome =
  | { readonly kind: "observed"; readonly cases: readonly string[] }
  | { readonly kind: "failed"; readonly case: string; readonly reason: string }
  | { readonly kind: "open"; readonly reason: string };
export class Receipt {
  readonly directory: string;
  constructor(directory: string) {
    this.directory = directory;
  }
  async begin(declaration: Declaration): Promise<void> {
    await mkdir(this.directory, { mode: 0o700 });
    await this.save("tuple.json", declaration);
  }
  async save(name: string, value: unknown): Promise<void> {
    if (!/^[a-zA-Z0-9._-]+$/.test(name)) throw new Error("evidence filename required");
    await writeFile(`${this.directory}/${name}`, `${encode(value)}\n`, { mode: 0o600, flag: "wx" });
  }
  async record(kind: string, value: unknown): Promise<void> {
    await appendFile(
      `${this.directory}/observations.jsonl`,
      `${encode({ at: new Date().toISOString(), kind, value })}\n`,
      { mode: 0o600 },
    );
  }
  async equal(reference: string, actual: unknown, expected: unknown): Promise<void> {
    const passed = isDeepStrictEqual(actual, expected);
    await this.record("assertion", {
      reference,
      passed,
      actualSha256: createHash("sha256")
        .update(encode({ type: typeof actual, value: actual }))
        .digest("hex"),
      expectedSha256: createHash("sha256")
        .update(encode({ type: typeof expected, value: expected }))
        .digest("hex"),
    });
    if (!passed) throw new ObservationFailure(reference);
  }
  async require(reference: string, condition: boolean): Promise<void> {
    await this.equal(reference, condition, true);
  }
}
