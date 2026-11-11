// Placeholder S1 settings for the deploy tests: the private settings file's
// shape with fake values (no real account, host or identity).

import type { DeploySettings } from "../../scripts/worker_deploy.ts";

export const SETTINGS: DeploySettings = {
  accountId: "0123456789abcdef0123456789abcdef",
  workerName: "collaboration-central-s1-migration",
  hostname: "s1-migration.staging.example",
  compatibilityDate: "2026-08-21",
  access: {
    teamDomain: "team.example",
    ownerIdentity: "owner@example.test",
    sessionAudience: "session-aud",
    adminAudience: "admin-aud",
  },
  central: {
    registryConfig: { authoritativeReceiver: "01a16eaa-ae36-7eec-9698-e8c34d10238b" },
    deliveryConfig: { relayPeer: "01a1c140-0000-7000-8000-000000000001" },
    holderAnswerTimeout: '{"secs":20,"nanos":0}',
    copyAckWait: '{"secs":20,"nanos":0}',
  },
};
