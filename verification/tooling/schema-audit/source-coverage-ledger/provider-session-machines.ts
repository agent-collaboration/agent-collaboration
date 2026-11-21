import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/provider-session-machines/src/";

export const section: LedgerSection = {
  root: "crates/provider-session-machines",
  files: [
    {
      file: `${R}provider_model_reasoning.rs`,
      disposition: { kind: "defines", owners: ["router-extension"] },
      note: "ModelReasoningUnknown derives serde (:8); the validated descriptors carry decoded model metadata",
    },
    {
      file: `${R}provider_setting_choice.rs`,
      disposition: { kind: "none" },
      note: "Advertised value and label pair (:4)",
    },
    {
      file: `${R}provider_settings_catalog.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "The protocol-neutral catalogue converts the owner-accepted values into session_event_model::SessionSettings (:125)",
    },
  ],
  sendSites: [],
};
