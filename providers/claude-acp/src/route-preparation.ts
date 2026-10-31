import * as z from "zod/mini";
import type { SessionCreationPreparationResult } from "./model-metadata.ts";

const recordSchema = z.record(z.string(), z.unknown());
const environmentSchema = z.record(z.string(), z.optional(z.string()));
const hostRouteSchema = z.object({
  baseUrl: z.url().check(
    z.refine((value) => {
      try {
        return ["http:", "https:"].includes(new URL(value).protocol);
      } catch {
        return false;
      }
    }),
  ),
  localToken: z.string().check(z.minLength(1)),
});
export type HostClaudeRoute = Readonly<z.infer<typeof hostRouteSchema>>;

export function captureHostRoute(environment: NodeJS.ProcessEnv): HostClaudeRoute {
  const parsed = hostRouteSchema.safeParse({
    baseUrl: environment.ANTHROPIC_BASE_URL,
    localToken: environment.ANTHROPIC_AUTH_TOKEN,
  });
  if (!parsed.success) throw new Error("Qualified Claude Host route is unavailable");
  return Object.freeze(parsed.data);
}

function publicRecord(value: unknown): Record<string, unknown> | undefined {
  const parsed = recordSchema.safeParse(value);
  return parsed.success ? parsed.data : undefined;
}

function publicEnvironment(value: unknown): Record<string, string | undefined> | undefined {
  if (value === undefined) return {};
  const parsed = environmentSchema.safeParse(value);
  return parsed.success ? parsed.data : undefined;
}

export function prepareClaudeRoute(
  params: unknown,
  route: HostClaudeRoute,
  modelConfig: string | undefined,
): SessionCreationPreparationResult {
  const request = publicRecord(params);
  const metadata = request?._meta == null ? {} : publicRecord(request._meta);
  const claude = metadata?.claudeCode === undefined ? {} : publicRecord(metadata.claudeCode);
  if (request === undefined || metadata === undefined || claude === undefined)
    return { kind: "unsupported", reason: "MalformedMetadata" };
  const options = claude.options === undefined ? {} : publicRecord(claude.options);
  if (options === undefined) return { kind: "unsupported", reason: "UnsupportedSdkOptions" };

  // Stock0.81 validates the post-policy config even when caller settings win.
  let configuredModels: Record<string, unknown> = {};
  if (modelConfig) {
    try {
      const decoded: unknown = JSON.parse(modelConfig);
      const parsed = publicRecord(decoded);
      if (parsed === undefined) return { kind: "unsupported", reason: "UnsupportedSdkOptions" };
      configuredModels = parsed;
    } catch {
      return { kind: "unsupported", reason: "UnsupportedSdkOptions" };
    }
  }
  // Preserve the vendor's nullish caller-settings precedence and truthy projection.
  const fallback: Record<string, unknown> = {
    ...(configuredModels.modelOverrides ? { modelOverrides: configuredModels.modelOverrides } : {}),
    ...(configuredModels.availableModels
      ? { availableModels: configuredModels.availableModels }
      : {}),
  };
  const settings = options.settings == null ? fallback : publicRecord(options.settings);
  const environment = publicEnvironment(options.env);
  const settingsEnvironment = settings === undefined ? undefined : publicEnvironment(settings.env);
  if (settings === undefined || environment === undefined || settingsEnvironment === undefined)
    return { kind: "unsupported", reason: "UnsupportedSdkOptions" };

  const routeEnvironment = {
    ANTHROPIC_BASE_URL: route.baseUrl,
    ANTHROPIC_AUTH_TOKEN: route.localToken,
    CLAUDE_CODE_USE_BEDROCK: "0",
    CLAUDE_CODE_USE_VERTEX: "0",
    ANTHROPIC_BEDROCK_BASE_URL: "",
    ANTHROPIC_VERTEX_BASE_URL: "",
    ANTHROPIC_VERTEX_PROJECT_ID: "",
    CLOUD_ML_REGION: "",
    AWS_REGION: "",
    ANTHROPIC_API_KEY: "",
    CLAUDE_CODE_OAUTH_TOKEN: "",
    ANTHROPIC_CUSTOM_HEADERS: "",
  };
  return {
    kind: "prepared",
    params: {
      ...request,
      _meta: {
        ...metadata,
        claudeCode: {
          ...claude,
          options: {
            ...options,
            env: { ...environment, ...routeEnvironment },
            settings: {
              ...settings,
              apiKeyHelper: "",
              env: { ...settingsEnvironment, ...routeEnvironment },
            },
          },
        },
      },
    },
  };
}
