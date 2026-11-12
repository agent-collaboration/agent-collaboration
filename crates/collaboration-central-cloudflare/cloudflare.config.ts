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
      CENTRAL_REGISTRY_CONFIG: { type: "text", value: "REPLACE_REGISTRY_CONFIG_JSON" },
      // JSON with the deployment's UUIDv7 relayPeer; required and validated by central.
      CENTRAL_DELIVERY_CONFIG: { type: "text", value: "REPLACE_DELIVERY_CONFIG_JSON" },
      FLEET: { type: "durable-object", worker: "replace-worker-name", exportName: "FleetObject" },
    },
    exports: {
      FleetObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
