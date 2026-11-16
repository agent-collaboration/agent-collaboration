import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/acp-provider-adapters/src/";
const N12 = [{ part: "N12" }, { part: "N6" }] as const;

export const rows: InventoryRow[] = [
  {
    file: `${R}model_reasoning_metadata.rs`,
    line: 11,
    owner: "router-extension",
    kind: "rust-type",
    defines:
      "Annotation, VersionedAnnotation, AnnotatedModel, AnnotatedChoice: serde mirror of the Zod modelReasoningSchema (v1, deny_unknown_fields)",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}model_reasoning_metadata.rs`,
    line: 50,
    owner: "router-extension",
    kind: "content-parser",
    defines: "decode_claude_models: version check and mapping to ModelReasoningMetadata",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model_reasoning_metadata.rs`,
    line: 100,
    owner: "cursor-extra",
    kind: "rust-type",
    defines:
      "CursorModels, CursorModel, CursorOption, CursorChoice (L100-L125): hand-written partial Deserialize of the cursor/list_available_models response, applied straight to the raw response Value at L130: a second definition of that Cursor wire contract (Cursor has no authoritative definition to pin, a separate no-authoritative-definition finding)",
    usedBy: [...N12],
    duplicateGroup: "cursor-extras",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}model_reasoning_metadata.rs`,
    line: 127,
    owner: "cursor-extra",
    kind: "content-parser",
    defines:
      "decode_cursor_models and cursor_reasoning (:160): maps the already decoded CursorModels (the L100 decoder) to ModelReasoningMetadata, picking the effort or thought_level select option by id and category",
    usedBy: [...N12],
    duplicateGroup: "cursor-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}cursor_behavior.rs`,
    line: 21,
    owner: "cursor-extra",
    kind: "method-table",
    defines: "cursor_login auth method id and cursor/list_available_models method name",
    usedBy: [{ part: "N6" }],
    duplicateGroup: "cursor-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_behavior.rs`,
    line: 93,
    owner: "claude-extra",
    kind: "content-parser",
    defines:
      "reads _meta.contextCompaction {version:1, trigger?: manual|automatic} from tool updates (L89-L109): an absent trigger is CompactionTrigger::Unlabelled; any other version or trigger value is MalformedMetadata",
    usedBy: [{ part: "N6" }, { part: "N12" }],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_behavior.rs`,
    line: 179,
    owner: "router-extension",
    kind: "content-parser",
    defines:
      "classify_commands: reads _meta.codexRouter.skillStatus available|unavailable and per-row .kind skill (L170-L224), the receiving side of the single annotation that providers/claude-acp skill-metadata.ts:110,113 writes; no second independently defined annotation schema",
    usedBy: [{ part: "N12" }, { part: "N6" }],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_behavior.rs`,
    line: 247,
    owner: "router-extension",
    kind: "content-parser",
    defines: "router_annotation: reads the _meta.codexRouter object",
    usedBy: [{ part: "N12" }, { part: "N6" }],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_launch.rs`,
    line: 14,
    owner: "claude-extra",
    kind: "schema-mechanism",
    defines:
      "claude_metadata.mjs (bundled Zod and entry) loaded by include_str and run by node; the Rust and TypeScript sides share no generated schema",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "schema-mechanism",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}cursor_behavior.rs`,
    line: 158,
    owner: "cursor-extra",
    kind: "content-parser",
    defines:
      "classify_command: reads the Cursor available_commands description suffix by hand ( (project skill) Repo, (user skill) User, (builtin skill) and (skill) System; (team), (global), (project), (user) and copy-request-id are non-skills; anything else UnrecognizedMetadata)",
    usedBy: [...N12],
    duplicateGroup: "cursor-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_behavior.rs`,
    line: 51,
    owner: "claude-extra",
    kind: "approval-table",
    defines:
      "auto_permission_mode: Router auto permission realized by the Claude session mode id acceptEdits (AsksFirst, EscalationUnproven); Cursor realizes it with mode agent (cursor_behavior.rs:42-52, AutoNotOffered)",
    usedBy: [...N12],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}claude_behavior.rs`,
    line: 117,
    owner: "claude-extra",
    kind: "content-parser",
    defines:
      "prepare: rewrites the selected $skill token into a /<name><suffix> slash-command text block (142) and compact as the literal /compact prompt (165); Cursor rewrites each $name to /name in place (cursor_behavior.rs:67-107); token finder selected_skill_content.rs:29",
    usedBy: [...N12],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
];
