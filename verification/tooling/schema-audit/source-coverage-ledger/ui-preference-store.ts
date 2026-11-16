import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/ui-preference-store",
  files: [
    {
      file: "crates/ui-preference-store/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "UiPreferenceStore l12 local SQLite of opaque (preference_id, value) text rows (StoredUiPreference l16); no serde, no protocol shapes",
    },
  ],
  sendSites: [],
};
