// Public deploy template for the one-time migration Worker, in the cf CLI's
// config format (the same format as crates/collaboration-central-cloudflare/
// cloudflare.config.ts). It deploys under central's own name with production's
// five storage classes and bindings (fleet-ops production live-target.json),
// so it runs inside the existing objects. Placeholders only: the real name,
// account and Access values belong to the private ops configuration.
const storage = (exportName: string) => ({
  type: "durable-object" as const,
  worker: "replace-worker-name",
  exportName,
});
export default {
  worker: {
    name: "replace-worker-name",
    entrypoint: "build/worker.js",
    compatibilityDate: "2026-08-21",
    workersDev: false,
    previewUrls: false,
    env: {
      ACCESS_TEAM_DOMAIN: { type: "text", value: "REPLACE_ACCESS_TEAM_DOMAIN" },
      ACCESS_SESSION_AUD: { type: "text", value: "REPLACE_SESSION_APP_AUD" },
      ACCESS_ADMIN_AUD: { type: "text", value: "REPLACE_ADMIN_APP_AUD" },
      ACCESS_OWNER_IDENTITY: { type: "text", value: "REPLACE_OWNER_IDENTITY" },
      LEASE_REGISTRY: storage("LeaseRegistryObject"),
      CONNECTIONS: storage("ConnectionObject"),
      PROJECT_COPIES: storage("ProjectCopyObject"),
      FLEET: storage("FleetObject"),
      MAILBOXES: storage("MailboxObject"),
    },
    exports: {
      LeaseRegistryObject: { type: "durable-object", storage: "sqlite" },
      ConnectionObject: { type: "durable-object", storage: "sqlite" },
      ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      MailboxObject: { type: "durable-object", storage: "sqlite" },
      FleetObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
