// The staging seed Worker's deploy configuration, derived from a staging
// central target's own config (the cf CLI format). It refuses production's
// target outright, and sets the STAGING_SEED_TARGET binding to the staging
// Worker's name, which the Worker requires before it writes anything.

import {
  checkLiveTarget,
  type WorkerConfig,
} from "../../../tools/central-migration/scripts/deploy_configs.ts";

/** Production central's identity (fleet-ops production live-target.json): never a seed target. */
const PRODUCTION = {
  worker: "collaboration-central",
  domain: "central.agent-collaboration.com",
} as const;

export function seedWorkerConfig(staging: WorkerConfig, entrypoint: string): WorkerConfig {
  checkLiveTarget(staging);
  const domains = (staging.worker.domains as readonly string[] | undefined) ?? [];
  if (staging.worker.name === PRODUCTION.worker || domains.includes(PRODUCTION.domain)) {
    throw new Error("refusing to configure the staging seed Worker for production's central");
  }
  if (!/staging/.test(staging.worker.name)) {
    throw new Error(`refusing a target whose name doesn't say staging: ${staging.worker.name}`);
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
