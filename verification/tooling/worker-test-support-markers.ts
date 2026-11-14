// Strings only the central Worker's `test-support` feature compiles in: the storage
// boundary's workerd case routes, the SQLx driver's scenarios, and their Durable
// Object classes.
export const testSupportMarkers: readonly string[] = [
  "/test-support/storage/",
  "/test-support/workspace/",
  "/test/fail-alarm-batch",
  "/test/central-failing-upgrade",
  "fixture failing upgrade",
  "/test-support/object-clock-ids",
  "/test/object-clock-ids",
  "/test-support/call-store/",
  "/test-support/driver/",
  "DriverScenarioObject",
  "StorageCaseObject",
  "x-case-migrator",
];
