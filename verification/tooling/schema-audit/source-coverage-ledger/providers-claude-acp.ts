import type { LedgerSection } from "../schema-audit-types.ts";

const R = "providers/claude-acp/src/";
const producerGap =
  "model_reasoning_node_tap.rs runs this producer through model-metadata-producer.ts (decorateResult only) but records the _meta.codexRouter.modelReasoning value at RecordLayer::Body (:31-37) and the Rust decode as TypedOutcome (:198), never the ACP frame";
const unitTest = {
  kind: "excluded",
  reason:
    "TypeScript unit test run only by node --test src/**/*.test.ts (package.json scripts.test); not reachable from the bundle entry router-claude-entry.ts (build.ts:8)",
} as const;

export const section: LedgerSection = {
  root: "providers/claude-acp",
  files: [
    { file: `${R}entry-envelope.test.ts`, disposition: unitTest },
    {
      file: `${R}fixtures/public-library.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test fixture: stand-in Claude ACP library loaded only by entry-envelope.test.ts:22; carries a hand sample available_commands_update (:44-50) as test input, not product output",
      },
      note: "test-only stand-in for the Claude ACP library used by the *.test.ts files",
    },
    {
      file: `${R}fixtures/public-model-library.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test fixture: stand-in library loaded only by model-entry-envelope.test.ts:25 with fake sessions, modelInfos and configOptions",
      },
      note: "test-only stand-in library with fake sessions and modelInfos",
    },
    {
      file: `${R}fixtures/public-route-library.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test fixture: stand-in library loaded only by route-entry-envelope.test.ts:71 and private-file-entry.test.ts:83,169; records the delegated _meta.claudeCode.options it receives",
      },
      note: "test-only stand-in library recording session method calls",
    },
    {
      file: `${R}fixtures/public-sdk.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test fixture: stand-in resolveSettings loaded only by the entry tests (entry-envelope.test.ts:23, model-entry-envelope.test.ts:26, route-entry-envelope.test.ts:72, private-file-entry.test.ts:84,170)",
      },
      note: "test-only stand-in for the Claude SDK resolveSettings",
    },
    {
      file: `${R}metadata-producer.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test driver run only by the Rust integration test acp-provider-adapters tests/integration/claude_producer_decoder.rs:50; not reachable from the bundle entry (build.ts:8)",
      },
      note: "runs SkillMetadataDecorator on one hand-written available_commands_update (:21-27) and prints the last annotated notification",
    },
    { file: `${R}model-entry-envelope.test.ts`, disposition: unitTest },
    {
      file: `${R}model-metadata-producer.ts`,
      disposition: {
        kind: "excluded",
        reason:
          "test driver run only by the Rust integration tests claude_producer_decoder.rs:9 and model_reasoning_node_tap.rs:51; not reachable from the bundle entry (build.ts:8)",
      },
      note: "runs ModelMetadataDecorator.decorateResult on one hand-written session (:2-14) and prints the decorated result",
    },
    { file: `${R}model-metadata.test.ts`, disposition: unitTest },
    {
      file: `${R}model-metadata.ts`,
      disposition: { kind: "defines", owners: ["router-extension", "claude-extra"] },
      note: "Zod modelReasoningSchema (:34), reasonSchema (:7), modelSchema (:15); writes _meta.codexRouter.modelReasoning on session results (:162) and config_option_update notifications (:178); wraps newSession, loadSession, resumeSession, setSessionConfigOption (:195-232)",
    },
    { file: `${R}private-file-entry.test.ts`, disposition: unitTest },
    { file: `${R}route-entry-envelope.test.ts`, disposition: unitTest },
    {
      file: `${R}route-preparation.ts`,
      disposition: { kind: "converts", owners: ["claude-extra"] },
      note: "rewrites the incoming session params _meta.claudeCode.options.env and .settings (:54-125) with Zod record schemas",
    },
    {
      file: `${R}router-claude-entry.ts`,
      disposition: { kind: "sends", owners: ["router-extension"] },
      note: "wraps client.sessionUpdate (:125-129): model decoration, then the skill decorator, whose send writes through the original sessionUpdate (:122); undecodable notifications pass through unchanged (:129); Zod only for the launch arguments and SDK policy env (:24, :31), outside the four protocols",
    },
    { file: `${R}settings-files.test.ts`, disposition: unitTest },
    {
      file: `${R}settings-files.ts`,
      disposition: { kind: "none" },
      note: "writes private temporary settings files; no protocol message",
    },
    { file: `${R}skill-metadata.test.ts`, disposition: unitTest },
    {
      file: `${R}skill-metadata.ts`,
      disposition: { kind: "defines", owners: ["router-extension"] },
      note: "Zod descriptor, reload, command and notification schemas (:3-25); writes _meta.codexRouter.skillStatus and row .kind (:110-113); sends each notification unchanged first (:128), then the annotated available_commands_update (:132, :172, :191, :197)",
    },
  ],
  sendSites: [
    {
      file: `${R}model-metadata.ts`,
      line: 162,
      edge: "acp-agent",
      sends:
        "session/new, load, resume and set_config_option results carrying _meta.codexRouter.modelReasoning",
      owner: { part: "N12" },
      origin: "product",
      coverage: { kind: "uncovered", reason: producerGap },
    },
    {
      file: `${R}model-metadata.ts`,
      line: 178,
      edge: "acp-agent",
      sends: "session/update config_option_update carrying _meta.codexRouter.modelReasoning",
      owner: { part: "N12" },
      origin: "product",
      coverage: { kind: "uncovered", reason: producerGap },
    },
    {
      file: `${R}skill-metadata.ts`,
      line: 191,
      edge: "acp-agent",
      sends:
        "session/update available_commands_update with _meta.codexRouter.skillStatus and row kind",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap covers the skill annotation; only unit tests in skill-metadata.test.ts and the Rust decoders' own tests",
      },
    },
    {
      file: `${R}skill-metadata.ts`,
      line: 128,
      edge: "acp-agent",
      sends:
        "every session/update passed through unchanged before annotation (includes the config_option_update decorated by model-metadata.ts:178)",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
    {
      file: `${R}skill-metadata.ts`,
      line: 132,
      edge: "acp-agent",
      sends:
        "available_commands_update annotated skillStatus unavailable when the session has no query",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
    {
      file: `${R}skill-metadata.ts`,
      line: 172,
      edge: "acp-agent",
      sends:
        "available_commands_update annotated skillStatus unavailable after a repeated failed input",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
    {
      file: `${R}skill-metadata.ts`,
      line: 197,
      edge: "acp-agent",
      sends:
        "available_commands_update annotated skillStatus unavailable when supportedCommands fails",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
    {
      file: `${R}router-claude-entry.ts`,
      line: 122,
      edge: "acp-agent",
      sends: "the transport write of every skill-decorator send: the original client.sessionUpdate",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
    {
      file: `${R}router-claude-entry.ts`,
      line: 129,
      edge: "acp-agent",
      sends: "session/update that fails the skill notification schema, passed through unchanged",
      owner: { part: "N12" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap records this send; claude_producer_decoder.rs runs metadata-producer.ts and decodes its printed output, no frame recorded",
      },
    },
  ],
};
