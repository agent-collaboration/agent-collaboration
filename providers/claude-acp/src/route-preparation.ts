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

const workRootLayerSchema = z.object({
  permissions: z.object({ additionalDirectories: z.array(z.string()) }),
  hooks: z.object({ PreToolUse: z.array(z.unknown()) }),
});
type WorkRootLayer = z.infer<typeof workRootLayerSchema>;

/// The Router's work-root layer for this session (D1 R18), from
/// _meta.codexRouter.workRootSettings; "malformed" when present but
/// unreadable; "missing" when the Router marked an auto start
/// (_meta.codexRouter.startMode) and sent no layer: such a session must not
/// start without its boundary.
function workRootLayer(
  metadata: Record<string, unknown>,
): WorkRootLayer | undefined | "malformed" | "missing" {
  const router =
    metadata.codexRouter === undefined ? undefined : publicRecord(metadata.codexRouter);
  if (metadata.codexRouter !== undefined && router === undefined) return "malformed";
  if (router?.startMode !== undefined && router.startMode !== "auto") return "malformed";
  if (router?.workRootSettings === undefined)
    return router?.startMode === "auto" ? "missing" : undefined;
  const parsed = workRootLayerSchema.safeParse(router.workRootSettings);
  return parsed.success ? parsed.data : "malformed";
}

/// Adds the Router's work roots and hook on top of the session's settings,
/// whichever won the vendor's precedence: its directories and PreToolUse
/// hooks are appended, never replacing the caller's or the owner's own.
function withWorkRootLayer(
  settings: Record<string, unknown>,
  layer: WorkRootLayer | undefined,
): Record<string, unknown> | undefined {
  if (layer === undefined) return settings;
  const permissions = settings.permissions === undefined ? {} : publicRecord(settings.permissions);
  const hooks = settings.hooks === undefined ? {} : publicRecord(settings.hooks);
  if (permissions === undefined || hooks === undefined) return undefined;
  const directories = permissions.additionalDirectories ?? [];
  const preToolUse = hooks.PreToolUse ?? [];
  if (!Array.isArray(directories) || !Array.isArray(preToolUse)) return undefined;
  return {
    ...settings,
    permissions: {
      ...permissions,
      additionalDirectories: [...directories, ...layer.permissions.additionalDirectories],
    },
    hooks: { ...hooks, PreToolUse: [...preToolUse, ...layer.hooks.PreToolUse] },
  };
}

function publicEnvironment(value: unknown): Record<string, string | undefined> | undefined {
  if (value === undefined) return {};
  const parsed = environmentSchema.safeParse(value);
  return parsed.success ? parsed.data : undefined;
}

interface RoutePreparationInputs {
  readonly params: unknown;
  readonly route: HostClaudeRoute;
  readonly modelConfig: string | undefined;
  readonly writeSettings: (contents: string) => string;
}

export function prepareClaudeRoute({
  params,
  route,
  modelConfig,
  writeSettings,
}: RoutePreparationInputs): SessionCreationPreparationResult {
  const request = publicRecord(params);
  const metadata = request?._meta == null ? {} : publicRecord(request._meta);
  const claude = metadata?.claudeCode === undefined ? {} : publicRecord(metadata.claudeCode);
  if (request === undefined || metadata === undefined || claude === undefined)
    return { kind: "unsupported", reason: "MalformedMetadata" };
  const options = claude.options === undefined ? {} : publicRecord(claude.options);
  if (options === undefined || options.sandbox !== undefined)
    return { kind: "unsupported", reason: "UnsupportedSdkOptions" };

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
  const layer = workRootLayer(metadata);
  if (layer === "malformed") return { kind: "unsupported", reason: "MalformedMetadata" };
  if (layer === "missing") return { kind: "unsupported", reason: "MissingWorkRootBoundary" };
  const callerSettings = options.settings == null ? fallback : publicRecord(options.settings);
  const settings =
    callerSettings === undefined ? undefined : withWorkRootLayer(callerSettings, layer);
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
  let serializedSettings: string;
  try {
    serializedSettings = JSON.stringify({
      ...settings,
      apiKeyHelper: "",
      env: { ...settingsEnvironment, ...routeEnvironment },
    });
  } catch {
    return { kind: "unsupported", reason: "UnsupportedSdkOptions" };
  }
  const settingsPath = writeSettings(serializedSettings);
  // The Router's own channel is consumed here; the vendor adapter never sees it.
  const { codexRouter: _consumed, ...vendorMetadata } = metadata;
  return {
    kind: "prepared",
    params: {
      ...request,
      _meta: {
        ...vendorMetadata,
        claudeCode: {
          ...claude,
          options: {
            ...options,
            env: { ...environment, ...routeEnvironment },
            settings: settingsPath,
          },
        },
      },
    },
  };
}
