// Test only: seed production storage through the old-central stand-in, then
// call the migration Worker over the same storage, one Worker per call.

import { ownerClaims, testSigner, type TestSigner } from "./owner_assertion.ts";
import { productionSeed, seed, snapshot, type ProductionSeed } from "./production_fixture.ts";
import { persistenceRoot, startWorker } from "./workerd_harness.ts";

export interface MigrationRun {
  readonly signer: TestSigner;
  readonly path: string;
  call(path: string, method?: string): Promise<{ status: number; body: Record<string, unknown> }>;
  oldStorage(): Promise<Record<string, unknown>>;
}

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
    await body({
      signer,
      path: storage.path,
      async call(path, method = "GET") {
        const worker = await startWorker("migration", storage.path, signer.jwks);
        try {
          const assertion = await signer.sign(ownerClaims(Math.floor(Date.now() / 1000)));
          const response = await worker.dispatchFetch(
            `https://central.test/admin/v1/migration/${path}`,
            {
              method,
              headers: { "Cf-Access-Jwt-Assertion": assertion },
            },
          );
          return {
            status: response.status,
            body: (await response.json()) as Record<string, unknown>,
          };
        } finally {
          await worker.dispose();
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
  return JSON.parse(JSON.stringify(storage).replaceAll('\\"', ""));
}
