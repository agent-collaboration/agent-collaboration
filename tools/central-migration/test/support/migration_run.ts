// Test only: seed production storage through the old-central stand-in, then
// call the migration Worker over the same storage, one Worker per call.

import { parseExact, stringifyExact } from "../../src/exact_json.ts";
import { ownerClaims, testSigner, type TestSigner } from "./owner_assertion.ts";
import {
  productionSeed,
  run as runStatements,
  seed,
  snapshot,
  type ProductionSeed,
} from "./production_fixture.ts";
import { persistenceRoot, startWorker } from "./workerd_harness.ts";

export interface CallReply {
  readonly status: number;
  readonly body: Record<string, unknown>;
}

export interface MigrationRun {
  readonly signer: TestSigner;
  readonly path: string;
  readonly production: ProductionSeed;
  /** One owner call; migrate carries the fixture's Mac-file facts unless `body` is given. */
  call(path: string, method?: string, body?: unknown): Promise<CallReply>;
  /** The real migrate or revert, at a fixed run time, interrupted after `afterWrites` object writes. */
  interrupted(
    call: "migrate" | "revert",
    options: { readonly afterWrites: number; readonly runTime: number },
  ): Promise<CallReply>;
  oldStorage(): Promise<Record<string, unknown>>;
  /** Runs SQL in one object through the old-central stand-in, to damage storage on purpose. */
  standInRun(binding: string, name: string, statements: readonly string[]): Promise<void>;
}

const reply = async (response: {
  status: number;
  text(): Promise<string>;
}): Promise<CallReply> => ({
  status: response.status,
  body: parseExact(await response.text()) as Record<string, unknown>,
});

/** Seeds production storage, then runs `body` with the migration Worker over it. */
export async function withProduction(
  edit: (production: ProductionSeed) => void,
  body: (run: MigrationRun) => Promise<void>,
): Promise<void> {
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    const production = productionSeed();
    edit(production);
    const standIn = await startWorker("old-central-stand-in", storage.path, signer.jwks);
    try {
      await seed(standIn, production);
    } finally {
      await standIn.dispose();
    }
    const macFilesBody = () => stringifyExact({ macFiles: production.macFiles });
    /** PD7's migrate body: the facts plus the run time and digest of the inventory just taken. */
    const migrateBody = (inventory: CallReply) =>
      stringifyExact({
        macFiles: production.macFiles,
        ...(typeof inventory.body.candidateDigest === "string"
          ? {
              runTime: inventory.body.runTimeMillis,
              candidateDigest: inventory.body.candidateDigest,
            }
          : {}),
      });
    const ownerCall = async (path: string, method: string, text: string | undefined) => {
      const worker = await startWorker("migration", storage.path, signer.jwks);
      try {
        const assertion = await signer.sign(ownerClaims(Math.floor(Date.now() / 1000)));
        const response = await worker.dispatchFetch(
          `https://central.test/admin/v1/migration/${path}`,
          {
            method,
            headers: { "Cf-Access-Jwt-Assertion": assertion },
            ...(text === undefined ? {} : { body: text }),
          },
        );
        return await reply(response);
      } finally {
        await worker.dispose();
      }
    };
    await body({
      signer,
      path: storage.path,
      production,
      async call(path, method = "GET", requestBody) {
        // A migrate without an explicit body runs as PD7 does: inventory first, then
        // migrate bound to that inventory's candidate.
        if (path === "migrate" && requestBody === undefined) {
          const inventory = await ownerCall("inventory", "POST", macFilesBody());
          return await ownerCall("migrate", method, migrateBody(inventory));
        }
        const worker = await startWorker("migration", storage.path, signer.jwks);
        try {
          const assertion = await signer.sign(ownerClaims(Math.floor(Date.now() / 1000)));
          const text = requestBody !== undefined ? stringifyExact(requestBody) : undefined;
          const response = await worker.dispatchFetch(
            `https://central.test/admin/v1/migration/${path}`,
            {
              method,
              headers: { "Cf-Access-Jwt-Assertion": assertion },
              ...(text === undefined ? {} : { body: text }),
            },
          );
          return await reply(response);
        } finally {
          await worker.dispose();
        }
      },
      async interrupted(call, { afterWrites, runTime }) {
        const worker = await startWorker("interrupted-migration", storage.path, signer.jwks, {
          INTERRUPT_AFTER_WRITES: String(afterWrites),
          RUN_TIME_MS: String(runTime),
        });
        try {
          const post = async (path: string, text: string) =>
            reply(
              await worker.dispatchFetch(`https://central.test/${path}`, {
                method: "POST",
                body: text,
              }),
            );
          if (call === "migrate") {
            // The inventory at this clock, then migrate bound to its candidate.
            return await post("migrate", migrateBody(await post("inventory", macFilesBody())));
          }
          return await post(call, macFilesBody());
        } finally {
          await worker.dispose();
        }
      },
      async standInRun(binding, name, statements) {
        const standInAgain = await startWorker("old-central-stand-in", storage.path, signer.jwks);
        try {
          await runStatements(standInAgain, binding, name, [...statements]);
        } finally {
          await standInAgain.dispose();
        }
      },
      async oldStorage() {
        const standInAgain = await startWorker("old-central-stand-in", storage.path, signer.jwks);
        try {
          return await snapshot(standInAgain);
        } finally {
          await standInAgain.dispose();
        }
      },
    });
  } finally {
    await storage.remove();
  }
}

/** SQLite quotes a renamed table's name in sqlite_schema; compare structure, not quoting. */
export function normalizedSchema(storage: Record<string, unknown>) {
  return parseExact(stringifyExact(storage).replaceAll('\\"', ""));
}
