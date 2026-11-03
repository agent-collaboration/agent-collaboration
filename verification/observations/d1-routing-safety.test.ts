import assert from "node:assert/strict";
import { test } from "node:test";
import { requireD1RoutingConfiguration } from "./d1-routing-safety.ts";

const profile = `model="gpt-6-luna"
model_provider="codex-router-debug"
cli_auth_credentials_store="ephemeral"
[features]
enable_request_compression=false
[model_providers.codex-router-debug]
name="OpenAI"
base_url="http://127.0.0.1:18787/v1"
wire_api="responses"
requires_openai_auth=false
supports_websockets=true
`;

test("D1 profile rejects real endpoints, client auth, credential carriers, alternate providers and fallback selection", (): void => {
  requireD1RoutingConfiguration(profile, "http://127.0.0.1:18787");
  for (const wrong of [
    profile.replace("18787/v1", "19741/v1"),
    profile.replace("http://127.0.0.1:18787/v1", "https://api.openai.com/v1"),
    profile.replace("requires_openai_auth=false", "requires_openai_auth=true"),
    profile.replace('model_provider="codex-router-debug"', 'model_provider="openai"'),
    `${profile}\nenv_key="UNEXPECTED"`,
    `${profile}\n[model_providers.openai]\nname="extra"`,
  ])
    assert.throws((): void => requireD1RoutingConfiguration(wrong, "http://127.0.0.1:18787"));
});
