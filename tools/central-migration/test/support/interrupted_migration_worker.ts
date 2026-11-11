// Test only. The real migrate and revert calls over the real storage objects,
// with two seams a test needs: a fixed run time, which production must not
// have, and the S1 interruption (src/fault_plan.ts) after a given number of
// object writes. Throwing there
// ends the request the way a lost Worker invocation does: every object written
// so far has committed its own transaction, and nothing after it has run.

import { InterruptedObjects } from "../../src/fault_plan.ts";
import { inventory, migrate, revert, type StorageNamespaces } from "../../src/migration_calls.ts";
import { parseExact, stringifyExact } from "../../src/exact_json.ts";
import { parseMacFiles, parseMigrateRequest } from "../../src/mac_file_evidence.ts";

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
