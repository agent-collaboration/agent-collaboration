export async function resolveSettings(
  options: unknown,
): Promise<{ effective: { env: Record<string, string> } }> {
  if (JSON.stringify(options) !== '{"settingSources":[]}')
    throw new Error("stock policy request changed");
  return { effective: { env: { ROUTER_METADATA_POLICY_FIXTURE: "applied" } } };
}
