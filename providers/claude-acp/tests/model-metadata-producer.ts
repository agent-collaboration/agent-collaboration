import { ModelMetadataDecorator } from "../src/model-metadata.ts";
const current = {
  configOptions: [],
  sessionIdentity: {},
  queryIdentity: {},
  models: {
    currentModelId: "sonnet",
    availableModels: [{ modelId: "sonnet" }, { modelId: "haiku" }],
  },
  modelInfos: [
    { value: "sonnet", supportsEffort: true, supportedEffortLevels: ["low", "high"] },
    { value: "haiku", supportsEffort: false },
  ],
};
const decorator = new ModelMetadataDecorator({ currentSession: () => current });
process.stdout.write(
  JSON.stringify(
    decorator.decorateResult({
      sessionId: "fixture-session",
      result: { sessionId: "fixture-session", configOptions: current.configOptions },
      source: { kind: "lifecycle" },
    }),
  ),
);
