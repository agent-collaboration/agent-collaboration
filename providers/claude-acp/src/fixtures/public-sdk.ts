export async function resolveSettings(
  options: unknown,
): Promise<{ effective: { env: Record<string, string> } }> {
  if (JSON.stringify(options) !== '{"settingSources":[]}')
    throw new Error("stock policy request changed");
  return {
    effective: {
      env: {
        ROUTER_METADATA_POLICY_FIXTURE: "applied",
        ANTHROPIC_BASE_URL: "https://policy.invalid/anthropic",
        ANTHROPIC_AUTH_TOKEN: "simulated-policy-token",
        ...(process.env.ROUTER_ROUTE_FIXTURE_MODEL_CONFIG === undefined
          ? {}
          : { CLAUDE_MODEL_CONFIG: process.env.ROUTER_ROUTE_FIXTURE_MODEL_CONFIG }),
      },
    },
  };
}
