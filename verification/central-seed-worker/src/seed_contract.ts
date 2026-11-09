// What the staging seed writes for one held project, as the driver sends it
// (staging-seed-worker-spec.md §4–5). Every u64 and i64 travels as decimal
// text; no integer passes through a JS number. A real travels as its exact
// shortest decimal text (serde_json's form), because JSON numbers lose -0.0;
// blobs travel as base64.

import type { SeedValue } from "./table_digest.ts";
import { canonicalInteger } from "./table_digest.ts";

/** One SqlValue on the wire to the Worker. */
export type TransportValue =
  | { readonly kind: "null" }
  | { readonly kind: "integer"; readonly value: string }
  | { readonly kind: "real"; readonly value: string }
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "blob"; readonly value: string };

export interface SeedPosition {
  readonly epoch: string;
  readonly sequence: string;
}

export interface SeedProject {
  /** D: the project database id. */
  readonly database: string;
  /** M: the current writer peer. */
  readonly holder: string;
  /** N: the writer's existing lease number. */
  readonly leaseNumber: string;
  /** S: the authoritative sender's registration start. */
  readonly start: SeedPosition;
  /** H: the public snapshot's position; the Copy is seeded at P = H. */
  readonly head: SeedPosition;
  /** T and E: Unix milliseconds. */
  readonly seededAt: string;
  readonly expiresAt: string;
  readonly schema: { readonly version: string; readonly fingerprint: string };
  readonly schemaSteps: readonly string[];
  readonly pages: readonly {
    readonly table: string;
    readonly rows: readonly (readonly TransportValue[])[];
  }[];
  readonly manifest: readonly {
    readonly table: string;
    readonly rowCount: string;
    readonly digest: string;
  }[];
}

/** Phase 1, in batches: each project's Copy, at its head image. */
export interface CopiesRequest {
  /** Must equal the Worker's STAGING_SEED_TARGET binding. */
  readonly target: string;
  readonly projects: readonly SeedProject[];
}

/** The facts the Registry rows carry for one project (§4). */
export type RegistryFacts = Pick<
  SeedProject,
  "database" | "holder" | "leaseNumber" | "seededAt" | "expiresAt"
>;

/** Phase 2, once, after every Copy is seeded: the Registry and the staging Fleet. */
export interface RegistryRequest {
  readonly target: string;
  /** I: a fresh staging Registry incarnation, independent of every N. */
  readonly registryIncarnation: string;
  readonly projects: readonly RegistryFacts[];
}

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const U64_MAX = 2n ** 64n - 1n;

export class SeedRefused extends Error {}
export const refuse = (where: string, why: string): never => {
  throw new SeedRefused(`${where}: ${why}`);
};

export const uuidV7 = (value: unknown, where: string): string =>
  typeof value === "string" && UUID_V7.test(value)
    ? value
    : refuse(where, "expected a canonical UUIDv7");

/** A canonical unsigned decimal, optionally required to be positive. */
export function unsigned(value: unknown, where: string, positive = false): string {
  if (typeof value !== "string" || !/^(0|[1-9]\d*)$/.test(value)) {
    return refuse(where, "expected canonical decimal text");
  }
  const number = BigInt(value);
  if (number > U64_MAX || (positive && number === 0n)) return refuse(where, "out of range");
  return value;
}

export function transportToSeed(value: TransportValue, where: string): SeedValue {
  switch (value.kind) {
    case "null":
      return { kind: "null" };
    case "integer":
      try {
        return { kind: "integer", value: canonicalInteger(value.value) };
      } catch {
        return refuse(where, "integer is not a canonical i64");
      }
    case "real": {
      if (typeof value.value !== "string") return refuse(where, "real must travel as decimal text");
      const real =
        value.value === "positiveInfinity"
          ? Number.POSITIVE_INFINITY
          : value.value === "negativeInfinity"
            ? Number.NEGATIVE_INFINITY
            : Number(value.value);
      if (Number.isNaN(real)) return refuse(where, "real is not a number");
      return { kind: "real", value: real };
    }
    case "text":
      return typeof value.value === "string"
        ? { kind: "text", value: value.value }
        : refuse(where, "text");
    case "blob": {
      if (typeof value.value !== "string") return refuse(where, "blob must be base64");
      const binary = atob(value.value);
      return { kind: "blob", value: Uint8Array.from(binary, (char) => char.charCodeAt(0)) };
    }
    default:
      return refuse(where, "unknown SqlValue kind");
  }
}

const compare = (left: SeedPosition, right: SeedPosition): number => {
  const epoch = BigInt(left.epoch) - BigInt(right.epoch);
  if (epoch !== 0n) return epoch < 0n ? -1 : 1;
  const sequence = BigInt(left.sequence) - BigInt(right.sequence);
  return sequence === 0n ? 0 : sequence < 0n ? -1 : 1;
};

/** The §5 invariants the receiver enforces (copy_contract.rs:5–14): S <= H and N >= H.epoch. */
export function checkProject(project: SeedProject): void {
  const where = `project ${String(project.database)}`;
  uuidV7(project.database, `${where}.database`);
  uuidV7(project.holder, `${where}.holder`);
  unsigned(project.leaseNumber, `${where}.leaseNumber`, true);
  for (const [name, position] of [
    ["start", project.start],
    ["head", project.head],
  ] as const) {
    unsigned(position.epoch, `${where}.${name}.epoch`, true);
    unsigned(position.sequence, `${where}.${name}.sequence`);
  }
  unsigned(project.seededAt, `${where}.seededAt`);
  unsigned(project.expiresAt, `${where}.expiresAt`);
  if (BigInt(project.expiresAt) < BigInt(project.seededAt))
    refuse(where, "expiry before seed time");
  if (compare(project.start, project.head) > 0) refuse(where, "start S is after head H");
  if (BigInt(project.leaseNumber) < BigInt(project.head.epoch))
    refuse(where, "lease number N is below H's epoch");
  unsigned(project.schema.version, `${where}.schema.version`);
  if (typeof project.schema.fingerprint !== "string" || project.schema.fingerprint === "") {
    refuse(where, "schema fingerprint missing");
  }
}
