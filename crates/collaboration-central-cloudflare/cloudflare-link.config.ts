// Public deploy template for central-link, in the cf CLI's config format: the
// private sign-in link's Worker. It has no route, no workers.dev or preview
// URL, and declares no Durable Object class or migration; its FLEET binding
// names central's existing Fleet class by central's deployed script name.
// Placeholders only; real names belong to the private ops configuration.
export default {
  worker: {
    name: "replace-link-worker-name",
    entrypoint: "build-link/index.js",
    compatibilityDate: "2026-08-21",
    workersDev: false,
    previewUrls: false,
    env: {
      CENTRAL_LEGACY_ORGANIZATION_ID: { type: "text", value: "REPLACE_LEGACY_ORGANIZATION_UUIDV7" },
      FLEET: {
        type: "durable-object",
        worker: "replace-central-worker-name",
        exportName: "FleetObject",
      },
    },
  },
};
