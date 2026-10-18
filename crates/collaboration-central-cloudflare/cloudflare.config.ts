// Public deploy template for the central Worker, in the cf CLI's config
// format. Placeholders only: the account, the dedicated custom domain and the
// real Access values belong only to the private ops configuration.
export default {
  worker: {
    name: "replace-worker-name",
    entrypoint: "build/index.js",
    compatibilityDate: "2026-08-21",
    workersDev: false,
    previewUrls: false,
    env: {
      ACCESS_TEAM_DOMAIN: { type: "text", value: "REPLACE_ACCESS_TEAM_DOMAIN" },
      ACCESS_SESSION_AUD: { type: "text", value: "REPLACE_SESSION_APP_AUD" },
      ACCESS_ADMIN_AUD: { type: "text", value: "REPLACE_ADMIN_APP_AUD" },
      ACCESS_OWNER_IDENTITY: { type: "text", value: "REPLACE_OWNER_IDENTITY" },
      CONNECTIONS: { type: "durable-object", worker: "replace-worker-name", exportName: "ConnectionObject" },
      PROJECT_COPIES: { type: "durable-object", worker: "replace-worker-name", exportName: "ProjectCopyObject" },
      LEASE_REGISTRY: { type: "durable-object", worker: "replace-worker-name", exportName: "LeaseRegistryObject" },
      FLEET: { type: "durable-object", worker: "replace-worker-name", exportName: "FleetObject" },
    },
    exports: {
      ConnectionObject: { type: "durable-object", storage: "sqlite" },
      ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      LeaseRegistryObject: { type: "durable-object", storage: "sqlite" },
      FleetObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
