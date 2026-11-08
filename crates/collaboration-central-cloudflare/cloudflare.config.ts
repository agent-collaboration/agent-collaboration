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
      CENTRAL_LEGACY_ORGANIZATION_ID: { type: "text", value: "REPLACE_LEGACY_ORGANIZATION_UUIDV7" },
      CENTRAL_REGISTRY_CONFIG: { type: "text", value: "REPLACE_REGISTRY_CONFIG_JSON" },
      // JSON with the deployment's UUIDv7 relayPeer; required and validated by central.
      CENTRAL_DELIVERY_CONFIG: { type: "text", value: "REPLACE_DELIVERY_CONFIG_JSON" },
      // The sign-in service that signs organization stamps, the audience its
      // stamps name for this central, and where it publishes its keys.
      CENTRAL_SIGN_IN_ISSUER: { type: "text", value: "REPLACE_SIGN_IN_ISSUER" },
      CENTRAL_SIGN_IN_AUDIENCE: { type: "text", value: "REPLACE_CENTRAL_STAMP_AUDIENCE" },
      CENTRAL_SIGN_IN_JWKS_URL: { type: "text", value: "REPLACE_SIGN_IN_JWKS_URL" },
      // Person tokens name this audience; DPoP proofs name this external origin.
      CENTRAL_ADMIN_AUDIENCE: { type: "text", value: "REPLACE_CENTRAL_ADMIN_AUDIENCE" },
      CENTRAL_PUBLIC_ORIGIN: { type: "text", value: "REPLACE_CENTRAL_PUBLIC_ORIGIN" },
      // The sign-in deployment's own UUIDv7 ServiceId, generated once.
      CENTRAL_SIGN_IN_SERVICE_ID: { type: "text", value: "REPLACE_SIGN_IN_SERVICE_UUIDV7" },
      PROJECT_COPIES: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "ProjectCopyObject",
      },
      FLEET: { type: "durable-object", worker: "replace-worker-name", exportName: "FleetObject" },
    },
    exports: {
      ProjectCopyObject: { type: "durable-object", storage: "sqlite" },
      FleetObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
