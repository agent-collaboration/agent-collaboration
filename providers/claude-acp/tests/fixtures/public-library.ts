interface FixtureNotification {
  sessionId: string;
  update: { sessionUpdate: string; availableCommands: { name: string; description: string }[] };
}
interface FixtureAgent {
  client: { sessionUpdate: (notification: FixtureNotification) => Promise<void> };
  sessions: Record<
    string,
    { query: { supportedCommands: () => Promise<unknown>; reloadSkills: () => Promise<unknown> } }
  >;
  dispose: () => Promise<void>;
}
export function runAcp(): { connection: { closed: Promise<void> }; agent: FixtureAgent } {
  if (process.env.ROUTER_METADATA_POLICY_FIXTURE !== "applied")
    throw new Error("managed environment not applied");
  const descriptor = { name: "fixture-review", description: "Review", argumentHint: "" };
  const agent: FixtureAgent = {
    client: {
      sessionUpdate: async (notification): Promise<void> => {
        process.stdout.write(`${JSON.stringify(notification)}\n`);
      },
    },
    sessions: {
      fixture: {
        query: {
          supportedCommands: async (): Promise<unknown> => [descriptor],
          reloadSkills: async (): Promise<unknown> => ({ skills: [descriptor] }),
        },
      },
    },
    dispose: async (): Promise<void> => {
      process.stderr.write("fixture-disposed\n");
    },
  };
  setImmediate((): void => {
    void agent.client.sessionUpdate({
      sessionId: "fixture",
      update: {
        sessionUpdate: "available_commands_update",
        availableCommands: [{ name: "fixture-review", description: "Review" }],
      },
    });
  });
  const closed = new Promise<void>((resolve): void => {
    process.stdin.once("end", resolve);
  });
  return { connection: { closed }, agent };
}
