import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-quota",
  files: [
    {
      file: "crates/codex-router-quota/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations snapshot/worker (:3-4) and package_name (:8); the rest is a cfg(test) mod tests (:12). No serde, no JSON, no message of the four protocols.",
    },
    {
      file: "crates/codex-router-quota/src/snapshot.rs",
      disposition: { kind: "none" },
      note: "in-memory quota model SnapshotSource (:7), QuotaRouteBand (:16), SnapshotFreshness (:46), QuotaSnapshot (:63) with freshness arithmetic; no Serialize/Deserialize, no JSON, not read from or written to any outside edge.",
    },
    {
      file: "crates/codex-router-quota/src/worker.rs",
      disposition: { kind: "none" },
      note: "refresh scheduling ports QuotaSnapshotReader (:9), QuotaRefreshSchedule (:16), RefreshScheduler (:24), QuotaRefreshRuntime (:71); in-process traits only, no serde, no JSON, no outside-edge message.",
    },
  ],
  sendSites: [],
};
