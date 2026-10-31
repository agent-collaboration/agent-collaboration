import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import * as z from "zod/mini";
import { SkillMetadataDecorator, notificationSchema } from "./skill-metadata.ts";
import type { MetadataQuery } from "./skill-metadata.ts";
import { ModelMetadataDecorator, decorateSessionMethods } from "./model-metadata.ts";
import type { ModelMetadataSession } from "./model-metadata.ts";
import { captureHostRoute, prepareClaudeRoute } from "./route-preparation.ts";

function isPublicObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
function publicObject(value: unknown): Record<string, unknown> {
  if (!isPublicObject(value)) throw new Error("Qualified Claude public API returned a non-object");
  return value;
}
function publicFunction(value: unknown): (...args: unknown[]) => unknown {
  if (typeof value !== "function")
    throw new Error("Qualified Claude public API is missing a function");
  return (...args: unknown[]): unknown => Reflect.apply(value, undefined, args);
}

const [libraryUrl, sdkUrl] = z
  .tuple([z.url().check(z.startsWith("file:")), z.url().check(z.startsWith("file:"))])
  .parse(process.argv.slice(1));
const hostRoute = captureHostRoute(process.env);
const sdk: unknown = await import(sdkUrl);
const resolveSettings = publicFunction(publicObject(sdk).resolveSettings);
const policy = z
  .object({ effective: z.object({ env: z.optional(z.record(z.string(), z.string())) }) })
  .parse(await resolveSettings({ settingSources: [] }));
for (const [key, value] of Object.entries(policy.effective.env ?? {})) process.env[key] = value;
console.log = console.error;
console.info = console.error;
console.warn = console.error;
console.debug = console.error;
process.on("unhandledRejection", (reason: unknown, promise: Promise<unknown>): void => {
  console.error("Unhandled Rejection at:", promise, "reason:", reason);
});
const logDirectory = process.env.CLAUDE_AGENT_LOGS;
function writeLog(...values: unknown[]): void {
  if (!logDirectory) return;
  const rendered = values
    .map((value) => (value instanceof Error ? (value.stack ?? value.message) : String(value)))
    .join(" ");
  appendFileSync(
    join(logDirectory, "agent.log"),
    `${new Date().toISOString()} pid=${process.pid} ${rendered}\n`,
  );
}
if (logDirectory) mkdirSync(logDirectory, { recursive: true });
const logger = logDirectory
  ? {
      log: writeLog,
      error: (...values: unknown[]): void => {
        console.error(...values);
        writeLog(...values);
      },
    }
  : undefined;
logger?.log("Claude ACP started");
const library: unknown = await import(libraryUrl);
const started = publicObject(publicFunction(publicObject(library).runAcp)(logger));
const agent = publicObject(started.agent);
const client = publicObject(agent.client);
const originalUpdate = client.sessionUpdate;
if (typeof originalUpdate !== "function")
  throw new Error("Qualified Claude client lacks sessionUpdate");
function modelSession(sessionId: string): ModelMetadataSession | undefined {
  const sessions = publicObject(agent.sessions);
  const candidate = sessions[sessionId];
  if (!isPublicObject(candidate) || candidate.queryClosed === true) return undefined;
  const queryIdentity = candidate.query;
  if (!isPublicObject(queryIdentity)) return undefined;
  return {
    sessionIdentity: candidate,
    queryIdentity,
    models: candidate.models,
    modelInfos: candidate.modelInfos,
    configOptions: candidate.configOptions,
  };
}
const modelMetadata = new ModelMetadataDecorator({ currentSession: modelSession });
decorateSessionMethods(agent, modelMetadata, (params) =>
  prepareClaudeRoute(params, hostRoute, process.env.CLAUDE_MODEL_CONFIG),
);
function currentQuery(sessionId: string): MetadataQuery | undefined {
  const sessions = publicObject(agent.sessions);
  const candidate = sessions[sessionId];
  if (typeof candidate !== "object" || candidate === null) return undefined;
  const session = publicObject(candidate);
  const queryIdentity = session.query;
  if (typeof queryIdentity !== "object" || queryIdentity === null) return undefined;
  const query = publicObject(queryIdentity);
  const supportedCommands = query.supportedCommands;
  const reloadSkills = query.reloadSkills;
  if (typeof supportedCommands !== "function" || typeof reloadSkills !== "function")
    return undefined;
  return {
    sessionIdentity: candidate,
    queryIdentity,
    supportedCommands: async (): Promise<unknown> => {
      const result: unknown = await Reflect.apply(supportedCommands, queryIdentity, []);
      return result;
    },
    reloadSkills: async (): Promise<unknown> => {
      const result: unknown = await Reflect.apply(reloadSkills, queryIdentity, []);
      return result;
    },
  };
}
const metadata = new SkillMetadataDecorator({
  currentQuery,
  send: async (notification): Promise<void> => {
    await Reflect.apply(originalUpdate, client, [notification]);
  },
});
client.sessionUpdate = async (notification: unknown): Promise<void> => {
  const decorated = modelMetadata.decorateNotification(notification);
  const decoded = notificationSchema.safeParse(decorated);
  if (decoded.success) await metadata.sessionUpdate(decoded.data);
  else await Reflect.apply(originalUpdate, client, [decorated]);
};
let shutdownTask: Promise<void> | undefined;
function shutdown(): Promise<void> {
  shutdownTask ??= (async (): Promise<void> => {
    const dispose = agent.dispose;
    if (typeof dispose !== "function") throw new Error("Qualified Claude agent lacks dispose");
    await Promise.resolve(Reflect.apply(dispose, agent, [])).catch((error: unknown): void => {
      console.error("Error during cleanup:", error);
    });
    process.exit(0);
  })();
  return shutdownTask;
}
const connection = publicObject(started.connection);
if (!(connection.closed instanceof Promise))
  throw new Error("Qualified Claude connection lacks closed promise");
void connection.closed.then(shutdown);
process.on("SIGTERM", (): void => {
  void shutdown();
});
process.on("SIGINT", (): void => {
  void shutdown();
});
process.stdin.resume();
