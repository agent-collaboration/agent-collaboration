import type { InventoryRow } from "../schema-audit-types.ts";

const R = "providers/claude-acp/src/";
const N12 = [{ part: "N12" }, { part: "N6" }] as const;

export const rows: InventoryRow[] = [
  {
    file: `${R}model-metadata.ts`,
    line: 34,
    owner: "router-extension",
    kind: "zod",
    defines:
      "modelReasoningSchema (v1 unknown or available) with reasonSchema, modelSchema, choiceSchema; the Rust serde mirror is acp-provider-adapters model_reasoning_metadata.rs:11",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model-metadata.ts`,
    line: 46,
    owner: "claude-extra",
    kind: "zod",
    defines:
      "modelInfoSchema and modelsSchema (:46-54): loose decoders of the Claude SDK session's models and modelInfos query values (supportsEffort, supportedEffortLevels, currentModelId, availableModels); Claude SDK inputs, not ACP wire",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model-metadata.ts`,
    line: 128,
    owner: "router-extension",
    kind: "hand-json",
    defines:
      "annotate: writes _meta.codexRouter.modelReasoning onto results (:162) and config_option_update (:178)",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model-metadata.ts`,
    line: 195,
    owner: "acp",
    kind: "method-table",
    defines:
      "list of wrapped agent methods: newSession, loadSession, resumeSession, setSessionConfigOption; sessionAddressSchema reads sessionId from params and results to address the decoration (195, 206-223)",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "acp-schema-copies",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}skill-metadata.ts`,
    line: 3,
    owner: "claude-extra",
    kind: "zod",
    defines:
      "descriptorSchema and reloadSchema (:3-11): loose decoders of the Claude SDK query's skill descriptors and reloadSkills result; Claude SDK inputs, not ACP wire",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}skill-metadata.ts`,
    line: 110,
    owner: "router-extension",
    kind: "hand-json",
    defines:
      "annotate: writes _meta.codexRouter.kind on rows and .skillStatus on the update (:110-115); its one receiver is acp-provider-adapters claude_behavior.rs:179: a Router extension producer and its receiver, not a second definition",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}route-preparation.ts`,
    line: 54,
    owner: "claude-extra",
    kind: "content-parser",
    defines:
      "reads and rewrites _meta.claudeCode.options (env, settings, sandbox refusal) on incoming session params; builds the route env (ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN and blanked provider keys, 87-100) and a settings JSON body written to a file whose path replaces options.settings (103-125); reads CLAUDE_MODEL_CONFIG modelOverrides and availableModels (63-80)",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "claude-extras",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model-metadata.ts`,
    line: 89,
    owner: "router-extension",
    kind: "content-parser",
    defines:
      "modelFacts: hand mapping of the upstream Claude session models.availableModels and modelInfos (supportsEffort, supportedEffortLevels) into modelReasoning rows: configId effort, a default choice {default, Default}, labels title-cased from the level (106-120), reasons Malformed, Ambiguous, Unavailable (92-103)",
    usedBy: [...N12],
    duplicateGroup: "router-extension-meta",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}model-metadata.ts`,
    line: 56,
    owner: "acp",
    kind: "zod",
    defines:
      "resultSchema {sessionId, _meta, configOptions} and notificationSchema {sessionId, update {sessionUpdate, _meta}} (:56-64): hand-written partial Zod copies of the ACP session result and session/update notification, parsed from the wrapped agent's results (:148) and notifications (:166) instead of the ACP library types",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "acp-schema-copies",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}skill-metadata.ts`,
    line: 12,
    owner: "acp",
    kind: "zod",
    defines:
      "commandSchema {name, description, _meta} and notificationSchema {sessionId, update {sessionUpdate, availableCommands, _meta}} (:12-24): hand-written partial Zod copies of the ACP available command and available_commands_update session/update notification",
    usedBy: [{ part: "N12" }],
    duplicateGroup: "acp-schema-copies",
    disposition: "duplicate",
    fixedBy: "D1",
  },
];
