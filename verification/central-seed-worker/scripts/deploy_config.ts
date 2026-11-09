// The staging seed Worker's deploy configuration, derived from a staging
// central target's own config (the cf CLI format). It refuses production's
// target outright, and sets the STAGING_SEED_TARGET binding to the staging
// Worker's name, which the Worker requires before it writes anything.
//
// The name itself isn't a guard: the seed deploys only as a first upload
// (tools/central-migration/scripts/worker_deploy.ts checkBefore refuses an
// existing Worker, its namespaces or its hostname), so it can never replace a
// live central, whatever the staging Worker is called.

import {
  checkLiveTarget,
  type WorkerConfig,
} from "../../../tools/central-migration/scripts/deploy_configs.ts";
import {
  PRODUCTION,
  canonicalName,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";

export { PRODUCTION };

export function seedWorkerConfig(staging: WorkerConfig, entrypoint: string): WorkerConfig {
  checkLiveTarget(staging);
  const domains = (staging.worker.domains as readonly string[] | undefined) ?? [];
  if (
    canonicalName(staging.worker.name) === PRODUCTION.worker ||
    domains.some((domain) => canonicalName(domain) === PRODUCTION.domain)
  ) {
    throw new Error("refusing to configure the staging seed Worker for production's central");
  }
  return {
    ...staging,
    worker: {
      ...staging.worker,
      entrypoint,
      env: {
        ...staging.worker.env,
        STAGING_SEED_TARGET: { type: "text", value: staging.worker.name },
      },
    },
  };
}
