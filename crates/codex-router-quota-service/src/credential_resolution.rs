//! Async credential-resolution port consumed by quota refresh.
use codex_router_auth::resolver::{CredentialResolverError, ResolvedProviderCredential};
use codex_router_core::ids::AccountId;
/// Async credential resolution used by native-async quota commands.
#[expect(
    async_fn_in_trait,
    reason = "quota callers use static dispatch with existing native async implementations"
)]
pub trait AsyncProviderCredentialResolver {
    async fn resolve_provider_credentials_async(
        &self,
        account_id: &AccountId,
        expected_provider: codex_router_core::provider::Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError>;

    async fn recover_unauthorized_credentials_async(
        &self,
        _account_id: &AccountId,
        _expected_provider: codex_router_core::provider::Provider,
        _rejected_generation: u64,
    ) -> Result<(ResolvedProviderCredential, bool), CredentialResolverError> {
        Err(CredentialResolverError::RefreshUnavailable)
    }
}
