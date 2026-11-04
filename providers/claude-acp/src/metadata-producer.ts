import { SkillMetadataDecorator } from "./skill-metadata.ts";
import type { MetadataNotification } from "./skill-metadata.ts";
const descriptor = {
  name: "fixture-review",
  description: "Fixture review",
  argumentHint: "<file>",
};
const query = {
  sessionIdentity: {},
  queryIdentity: {},
  supportedCommands: async (): Promise<unknown> => [descriptor],
  reloadSkills: async (): Promise<unknown> => ({ skills: [descriptor] }),
};
const messages: MetadataNotification[] = [];
const decorator = new SkillMetadataDecorator({
  currentQuery: () => query,
  send: async (message): Promise<void> => {
    messages.push(message);
  },
});
await decorator.sessionUpdate({
  sessionId: "fixture-session",
  update: {
    sessionUpdate: "available_commands_update",
    availableCommands: [{ name: descriptor.name, description: descriptor.description }],
  },
});
await decorator.settled();
process.stdout.write(JSON.stringify(messages.at(-1)));
