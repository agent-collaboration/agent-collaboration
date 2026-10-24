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
      CENTRAL_HOLDER_ANSWER_TIMEOUT: { type: "text", value: '{"secs":20,"nanos":0}' },
      CENTRAL_COPY_ACK_WAIT: { type: "text", value: '{"secs":20,"nanos":0}' },
      CENTRAL_REGISTRY_CONFIG: { type: "text", value: "REPLACE_REGISTRY_CONFIG_JSON" },
      // JSON with the deployment's UUIDv7 relayPeer; required and validated by central.
      CENTRAL_DELIVERY_CONFIG: { type: "text", value: "REPLACE_DELIVERY_CONFIG_JSON" },
      CONNECTIONS: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "ConnectionObject",
      },
      PROJECT_COPIES: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "ProjectCopyObject",
      },
      LEASE_REGISTRY: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "LeaseRegistryObject",
      },
      FLEET: { type: "durable-object", worker: "replace-worker-name", exportName: "FleetObject" },
      MAILBOXES: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "MailboxObject",
      },
    },
    exports: {
      ConnectionObject: { type: "durable-object", storage: "sqlite" },
      ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      LeaseRegistryObject: { type: "durable-object", storage: "sqlite" },
      FleetObject: { type: "durable-object", storage: "sqlite" },
      MailboxObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
