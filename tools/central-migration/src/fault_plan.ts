// The S1 staging interruption control (T10): an object-write count after which
// migrate or revert stops, the way a lost Worker invocation does. Every object
// written so far has committed its own transaction and nothing after it runs;
// Copies commit first and the Fleet last, so a small count stops after some
// Copies and before the Fleet.
//
// It can't reach production. Two guards hold it:
// - only the deploy config renders its bindings, and only for the S1 staging
//   allocation (worker_deploy.ts renderConfig); the guarded deploy refuses any
//   other config carrying them;
// - this Worker applies it only when the fault target names the Worker itself
//   (MIGRATION_WORKER_NAME, which every render sets from the target's own name).
// A resume is a redeploy without the hook, so it runs the production-shaped Worker.

import { StorageObjects, type StorageNamespaces } from "./migration_calls.ts";
import type { ApplyReply, RevertReply, Statement } from "./storage_objects.ts";

/** The bindings the staging render adds, and the one every render sets. */
export const FAULT_BINDINGS = {
  afterWrites: "MIGRATION_FAULT_AFTER_WRITES",
  target: "MIGRATION_FAULT_TARGET",
  workerName: "MIGRATION_WORKER_NAME",
} as const;

export interface FaultEnv {
  readonly MIGRATION_FAULT_AFTER_WRITES?: string;
  readonly MIGRATION_FAULT_TARGET?: string;
  readonly MIGRATION_WORKER_NAME?: string;
}

/** The only allocation the hook may run in, whatever bindings a deployment carries. */
export const HOOK_ALLOCATIONS: readonly string[] = ["collaboration-central-s1-migration"];

/** The write count to stop after, or null when this deployment carries no hook for itself. */
export function faultAfterWrites(env: FaultEnv): number | null {
  const target = env.MIGRATION_FAULT_TARGET;
  const self = env.MIGRATION_WORKER_NAME;
  if (target === undefined || self === undefined || target === "" || target !== self) return null;
  if (!HOOK_ALLOCATIONS.includes(self)) return null;
  const count = Number(env.MIGRATION_FAULT_AFTER_WRITES);
  return Number.isSafeInteger(count) && count >= 0 ? count : null;
}

/** StorageObjects that stop after `limit` object writes. */
export class InterruptedObjects extends StorageObjects {
  private writes = 0;
  private readonly limit: number;
  /** Each object write that committed before the stop, in order, with its reply. */
  readonly committed: {
    readonly object: string;
    readonly kind: string;
    readonly rowsWritten: number;
  }[] = [];
  private record<T extends { kind: string; rowsWritten: number }>(object: string, reply: T): T {
    this.committed.push({ object, kind: reply.kind, rowsWritten: reply.rowsWritten });
    return reply;
  }
  constructor(namespaces: StorageNamespaces, limit: number) {
    super(namespaces);
    this.limit = limit;
  }
  private next(): void {
    if (this.writes >= this.limit) throw new Interrupted(this.limit, this.committed);
    this.writes += 1;
  }
  override async applyCopy(project: string, statements: readonly Statement[]): Promise<ApplyReply> {
    this.next();
    return this.record(`copy ${project}`, await super.applyCopy(project, statements));
  }
  override async applyFleet(statements: readonly Statement[]): Promise<ApplyReply> {
    this.next();
    return this.record("fleet", await super.applyFleet(statements));
  }
  override async revertCopy(project: string): Promise<RevertReply> {
    this.next();
    return this.record(`copy ${project}`, await super.revertCopy(project));
  }
  override async revertFleet(): Promise<RevertReply> {
    this.next();
    return this.record("fleet", await super.revertFleet());
  }
}

/** The stop, with the object writes that committed before it, for reconciliation. */
export class Interrupted extends Error {
  readonly afterWrites: number;
  readonly committed: readonly {
    readonly object: string;
    readonly kind: string;
    readonly rowsWritten: number;
  }[];
  constructor(afterWrites: number, committed: Interrupted["committed"]) {
    super(`interrupted after ${afterWrites} object writes`);
    this.afterWrites = afterWrites;
    this.committed = [...committed];
  }
}

/** The Worker's storage objects: interrupted only under its own staging hook. */
export function storageObjectsFor(env: StorageNamespaces & FaultEnv): StorageObjects {
  const limit = faultAfterWrites(env);
  return limit === null ? new StorageObjects(env) : new InterruptedObjects(env, limit);
}
