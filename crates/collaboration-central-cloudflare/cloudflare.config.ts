// Public deploy template for the central Worker, in the cf CLI's config
// format. Placeholders only: the account, the dedicated custom domain and the
// real sign-in values belong only to the private ops configuration.
export default {
  worker: {
    name: "replace-worker-name",
    entrypoint: "build/index.js",
    compatibilityDate: "2026-08-21",
    workersDev: false,
    previewUrls: false,
    env: {
      CENTRAL: { type: "durable-object", worker: "replace-worker-name", exportName: "CentralObject" },
      // One store per workspace, named by its workspace id; it runs only the statements
      // central sends it.
      WORKSPACE_STORE: {
        type: "durable-object",
        worker: "replace-worker-name",
        exportName: "WorkspaceStoreObject",
      },
    },
    exports: {
      CentralObject: { type: "durable-object", storage: "sqlite" },
      WorkspaceStoreObject: { type: "durable-object", storage: "sqlite" },
    },
  },
};
