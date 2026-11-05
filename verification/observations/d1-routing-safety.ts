/** The private D1 native profile admits exactly one uncredentialed debug destination. */
import { parse } from "smol-toml";
import { z } from "zod";

const routingConfiguration = z
  .object({
    model: z.literal("gpt-6-luna"),
    model_provider: z.literal("codex-router-debug"),
    cli_auth_credentials_store: z.literal("ephemeral"),
    features: z.object({ enable_request_compression: z.literal(false) }).strict(),
    model_providers: z
      .object({
        "codex-router-debug": z
          .object({
            name: z.literal("OpenAI"),
            base_url: z.string(),
            wire_api: z.literal("responses"),
            requires_openai_auth: z.literal(false),
            supports_websockets: z.literal(true),
          })
          .strict(),
      })
      .strict(),
  })
  .strict();

export function requireD1RoutingConfiguration(text: string, origin: string): void {
  const config = routingConfiguration.parse(parse(text));
  const expected = new URL("/v1", origin);
  if (
    expected.protocol !== "http:" ||
    expected.hostname !== "127.0.0.1" ||
    ["19741", "19742", "8787", "8788"].includes(expected.port)
  )
    throw new Error("declared D1 debug origin is not isolated loopback");
  if (config.model_providers["codex-router-debug"].base_url !== expected.href)
    throw new Error("D1 native model endpoint differs from the declared debug origin");
}
