import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 0 colocated unit-test files (*_test.rs, *_tests.rs).

export const section: LedgerSection = {
  root: "crates/owned-process-test-support",
  files: [
    {
      file: "crates/owned-process-test-support/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and FixtureResult alias.",
    },
    {
      file: "crates/owned-process-test-support/src/owned_cli_process.rs",
      disposition: { kind: "none" },
      note: "Owned CLI child capture: piped stdout/stderr, deadline, kill and wait on failure (L40-L106); output kept as bytes, no message shape.",
    },
    {
      file: "crates/owned-process-test-support/src/scenario_finalization.rs",
      disposition: { kind: "none" },
      note: "Panic-safe scenario finalization and its error types (L16-L102); no message shape.",
    },
  ],
  sendSites: [],
};
