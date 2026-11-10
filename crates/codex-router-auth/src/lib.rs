//! OpenAI account authentication boundaries for codex-router.

pub mod claude_oauth;
pub mod credential_activation;
pub mod live_quota;
pub mod oauth;
pub mod openai_oauth;
pub mod quota_client;
pub mod refresh_worker;
pub mod resolver;
pub mod router_credentials;

/// Returns this crate's package name.
#[must_use]
pub const fn package_name() -> &'static str {
    "codex-router-auth"
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
