// S2's facts from each holder Mac, carried in migrate's request: what the
// retained project file says about its writer, read without opening a writer
// (verification.html S2). Migrate compares them with central's own records
// before any write, so a Mac registered with another receiver, or a copy whose
// acknowledged position the Mac no longer retains, stops the run (R7).

import { Invalid, epoch, struct, u64, uuidV7, vec, type Decoder } from "./serde_shapes.ts";

export interface ExactPosition {
  readonly epoch: bigint;
  readonly sequence: bigint;
}

export interface MacFileEvidence {
  /** The project the file belongs to. */
  readonly database: string;
  /** The writer the file is registered as, and its lease number. */
  readonly holder: string;
  readonly lease: bigint;
  /** The authoritative receiver the file's sender is registered with. */
  readonly senderReceiver: string;
  /** The positions the file still holds and can resend: floor through head. */
  readonly retained: { readonly floor: ExactPosition; readonly head: ExactPosition };
}

const position = struct({ epoch, sequence: u64 }, true);
const evidence: Decoder<MacFileEvidence> = struct(
  {
    database: uuidV7,
    holder: uuidV7,
    lease: epoch,
    senderReceiver: uuidV7,
    retained: struct({ floor: position, head: position }, true),
  },
  true,
);
const request = struct({ macFiles: vec(evidence) }, true);

export const positionBefore = (a: ExactPosition, b: ExactPosition): boolean =>
  a.epoch < b.epoch || (a.epoch === b.epoch && a.sequence < b.sequence);

/** Migrate's body, or an error naming the first malformed field. */
export function parseMacFiles(body: unknown): { readonly files: readonly MacFileEvidence[] } {
  try {
    const { macFiles } = request(body, "body");
    const seen = new Set<string>();
    for (const file of macFiles) {
      if (seen.has(file.database)) throw new Error(`project ${file.database} appears twice`);
      seen.add(file.database);
      if (positionBefore(file.retained.head, file.retained.floor))
        throw new Error(`project ${file.database}: retained head is before its floor`);
    }
    return { files: macFiles };
  } catch (error) {
    if (error instanceof Invalid) throw new Error(error.message);
    throw error;
  }
}
