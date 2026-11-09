// Test only. The real migrate and revert calls over the real storage objects,
// with two seams a test needs and production must not have: a fixed run time,
// and an interruption after a given number of object writes. Throwing there
// ends the request the way a lost Worker invocation does: every object written
// so far has committed its own transaction, and nothing after it has run.

import {
  StorageObjects,
  inventory,
  migrate,
  revert,
  type StorageNamespaces,
} from "../../src/migration_calls.ts";
import { parseExact, stringifyExact } from "../../src/exact_json.ts";
import { parseMacFiles, parseMigrateRequest } from "../../src/mac_file_evidence.ts";
import type { ApplyReply, RevertReply, Statement } from "../../src/storage_objects.ts";

export {
  ConnectionObject,
  FleetObject,
  LeaseRegistryObject,
  MailboxObject,
  ProjectCopyObject,
} from "../../src/storage_objects.ts";

interface TestEnv extends StorageNamespaces {
  readonly CENTRAL_REGISTRY_CONFIG: string;
  /** Object writes that commit before the interruption. */
  readonly INTERRUPT_AFTER_WRITES: string;
  /** The run's instant, milliseconds since the epoch. */
  readonly RUN_TIME_MS: string;
}

class InterruptedObjects extends StorageObjects {
  private writes = 0;
  private readonly limit: number;
  constructor(namespaces: StorageNamespaces, limit: number) {
    super(namespaces);
    this.limit = limit;
  }
  private next(): void {
    if (this.writes >= this.limit) throw new Error(`interrupted after ${this.limit} object writes`);
    this.writes += 1;
  }
  override applyCopy(project: string, statements: readonly Statement[]): Promise<ApplyReply> {
    this.next();
    return super.applyCopy(project, statements);
  }
  override applyFleet(statements: readonly Statement[]): Promise<ApplyReply> {
    this.next();
    return super.applyFleet(statements);
  }
  override revertCopy(project: string): Promise<RevertReply> {
    this.next();
    return super.revertCopy(project);
  }
  override revertFleet(): Promise<RevertReply> {
    this.next();
    return super.revertFleet();
  }
}

export default {
  async fetch(request: Request, env: TestEnv): Promise<Response> {
    const objects = new InterruptedObjects(env, Number(env.INTERRUPT_AFTER_WRITES));
    const context = {
      now: () => Number(env.RUN_TIME_MS),
      registryConfig: env.CENTRAL_REGISTRY_CONFIG,
    };
    try {
      const path = new URL(request.url).pathname;
      const answer =
        path === "/inventory"
          ? await inventory(objects, context, parseMacFiles(parseExact(await request.text())).files)
          : path === "/migrate"
            ? await migrate(objects, context, parseMigrateRequest(parseExact(await request.text())))
            : await revert(objects);
      return new Response(stringifyExact(answer.body), { status: answer.status });
    } catch (error) {
      return new Response(stringifyExact({ interrupted: String(error) }), { status: 500 });
    }
  },
};
