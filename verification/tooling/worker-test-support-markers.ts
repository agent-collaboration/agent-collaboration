// Strings only the central Worker's `test-support` feature compiles in: the storage
// boundary's workerd case routes and their Durable Object class.
export const testSupportMarkers: readonly string[] = [
  "/test-support/storage/",
  "/test-support/call-store/",
  "StorageCaseObject",
  "x-case-migrator",
];
