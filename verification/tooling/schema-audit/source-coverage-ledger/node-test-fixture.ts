import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 0 colocated unit-test files (*_test.rs, *_tests.rs).

export const section: LedgerSection = {
  root: "crates/node-test-fixture",
  files: [
    {
      file: "crates/node-test-fixture/src/lib.rs",
      disposition: { kind: "none" },
      note: "Locates node and verification/fixtures programs (L9-L25) and writes a launcher script that passes the caller's serde_json::Value configuration through unchanged to runFixture (L27-L49); it defines no message shape and sends nothing itself.",
    },
  ],
  sendSites: [],
};
