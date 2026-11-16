import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/acp-provider-adapters/src/";

export const section: LedgerSection = {
  root: "crates/acp-provider-adapters",
  files: [
    {
      file: `${R}claude_behavior.rs`,
      disposition: { kind: "converts", owners: ["claude-extra", "router-extension"] },
      note: "reads _meta.codexRouter.skillStatus and .kind (:179, :201, :247) and _meta.contextCompaction (:93-110) into Router SkillView and compaction hints; auto permission realized by mode acceptEdits (:51-62); /<skill> and /compact prompt text (:117-168); model decode delegates to model_reasoning_metadata.rs",
    },
    {
      file: `${R}claude_launch.rs`,
      disposition: { kind: "none" },
      note: "serde shapes outside the four protocols: PackageManifest (:40) decodes the package.json of claude-agent-acp for launch admission and QualifiedImports (:48) decodes the qualifying node script stdout (:157); pins claude-agent-acp 0.81.0 (:120) and SDK 0.3.280 (:215); include_str of the bundle (:14)",
    },
    {
      file: `${R}claude_metadata.mjs`,
      disposition: {
        kind: "excluded",
        reason:
          "generated bundle of providers/claude-acp/src built by build.ts (bundled Zod plus the entry); every definition is anchored in the TypeScript sources, see providers-claude-acp",
      },
    },
    {
      file: `${R}cursor_behavior.rs`,
      disposition: { kind: "defines", owners: ["cursor-extra"] },
      note: "method names cursor_login (:21) and cursor/list_available_models (:33); auto permission realized by mode agent (:42-52); /name slash-command rewrite of selected skills (:67-107); command description-suffix skill classifier (:158-193)",
    },
    { file: `${R}lib.rs`, disposition: { kind: "none" }, note: "module list" },
    {
      file: `${R}model_reasoning_metadata.rs`,
      disposition: { kind: "defines", owners: ["router-extension", "cursor-extra"] },
      note: "Claude _meta.codexRouter.modelReasoning v1 serde mirror of the Zod schema (:11-48, decode :50); Cursor cursor/list_available_models response structs (:100-125, decode :127)",
    },
    {
      file: `${R}selected_skill_content.rs`,
      disposition: { kind: "none" },
      note: "finds $skill tokens in Router prompt text (:29-39) and the one text block (:7-27); helper of the slash-command rewrite inventoried at claude_behavior.rs:117; defines no shape",
    },
    {
      file: `${R}standard_behavior.rs`,
      disposition: { kind: "none" },
      note: "default behavior: no metadata source, no auto mode, no skill classification (:9-59); no shape",
    },
  ],
  sendSites: [],
};
