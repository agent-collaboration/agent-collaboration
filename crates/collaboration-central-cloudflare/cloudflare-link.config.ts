// Private sign-in Worker, built with --features link into build-link/.
// It has no public route, workers.dev URL, preview, DO export or migration.
// CENTRAL names the existing CentralObject on the public central Worker.
// Placeholders only; deployed names belong to the private ops configuration.
export default {
  worker: {
    name: "replace-link-worker-name",
    entrypoint: "build-link/index.js",
    compatibilityDate: "2026-08-21",
    workersDev: false,
    previewUrls: false,
    env: {
      CENTRAL: {
        type: "durable-object",
        worker: "replace-central-worker-name",
        exportName: "CentralObject",
      },
    },
  },
};
