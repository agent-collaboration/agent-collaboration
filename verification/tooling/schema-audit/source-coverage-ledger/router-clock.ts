import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/router-clock",
  files: [
    {
      file: "crates/router-clock/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "ClockInstant l16 / RuntimeClock trait l52 injected time capability; no serde, no protocol shapes",
    },
    {
      file: "crates/router-clock/src/manual_clock.rs",
      disposition: {
        kind: "none",
      },
      note: "ManualClock l24 shared test time; no serde, no protocol shapes",
    },
    {
      file: "crates/router-clock/src/system_runtime_clock.rs",
      disposition: {
        kind: "none",
      },
      note: "SystemRuntimeClock l7, the one native time access; no protocol shapes",
    },
  ],
  sendSites: [],
};
