import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-auth",
  files: [
    {
      file: "crates/codex-router-auth/src/claude_oauth.rs",
      disposition: { kind: "none" },
      note: "Anthropic OAuth token endpoint: Serialize ClaudeAuthorizationCodeRequest/ClaudeRefreshTokenRequest (418-433) sent by reqwest (204,271) and Deserialize ClaudeOAuthTokenResponse/ClaudeOAuthErrorResponse (435-446) decoded at 221,320,333; Anthropic OAuth HTTP is not one of the four protocols nor the six edges; cfg(test) mod tests at 455-456",
    },
    {
      file: "crates/codex-router-auth/src/credential_activation.rs",
      disposition: { kind: "none" },
      note: "login activation of a staged credential through Router state and secret store (19-343); no serde, no message shape",
    },
    {
      file: "crates/codex-router-auth/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations and package_name (1-17); the rest is the #[cfg(test)] mod tests declared at 19-20",
    },
    {
      file: "crates/codex-router-auth/src/live_quota.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Codex auth.json; ChatGPT /wham/usage endpoint)",
      },
      note: "reads Codex-owned auth.json through stand-in Deserialize StoredAuth {auth_mode,tokens,OPENAI_API_KEY} and StoredTokens {access_token} (40-53), decoded at 258 and checked at 264-283; the same kind of stand-in as Host auth_file_reload.rs:146-156,173. Also Deserialize UsageResponse/WindowPair/UsageWindow (69-111) for the ChatGPT /wham/usage endpoint (18-26), which is not one of the four protocols; cfg(test) mod tests at 356-357",
    },
    {
      file: "crates/codex-router-auth/src/oauth.rs",
      disposition: { kind: "none" },
      note: "OAuth token status and HTTP status classification enums (5-81); no serde, no message shape",
    },
    {
      file: "crates/codex-router-auth/src/openai_oauth.rs",
      disposition: { kind: "none" },
      note: "OpenAI OAuth device-code flow: Serialize UserCodeRequest/TokenPollRequest/AuthorizationCodeExchangeRequest and Deserialize UserCodeResponse/AuthorizationCodeResponse/TokenExchangeResponse/IdTokenClaims (450-507), sent and decoded at 253,288,347,380,403,446; OpenAI auth.openai.com HTTP is not one of the four protocols; cfg(test) tests module at 516-518",
    },
    {
      file: "crates/codex-router-auth/src/quota_client.rs",
      disposition: { kind: "none" },
      note: "authenticated quota fetch facade (QuotaFetchRequest/Response, 7-85) without serde; no message shape",
    },
    {
      file: "crates/codex-router-auth/src/refresh_worker.rs",
      disposition: { kind: "none" },
      note: "background refresh planning (AccountRefreshInput, RefreshWorkDecision, 8-80) without serde; no message shape",
    },
    {
      file: "crates/codex-router-auth/src/resolver.rs",
      disposition: { kind: "none" },
      note: "credential resolver; OpenAI OAuth refresh endpoint: Serialize RefreshTokenRequest (481) sent at 375, Deserialize RefreshTokenResponse/RefreshTokenErrorResponse (488-500) decoded at 424,445; not one of the four protocols; cfg(test) items at 234,277,567-617,702-714,807",
    },
    {
      file: "crates/codex-router-auth/src/resolver/credential_renewal.rs",
      disposition: { kind: "none" },
      note: "async credential lease and storage renewal workflow (15-811); no serde, no message shape",
    },
    {
      file: "crates/codex-router-auth/src/router_credentials.rs",
      disposition: { kind: "none" },
      note: "in-memory RouterCredentialBundle DTO with redacted Debug (7-65); no serde, no message shape",
    },
  ],
  sendSites: [],
};
