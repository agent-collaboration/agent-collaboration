//! Select the live acceptance registry before using the shared Rust peer receiver.
use super::proof_context::{ProofContext, ProofResult};
pub(super) use codex_router_test_support::owned_peer_fixture::PeerFixture;

pub(super) async fn start(proof: &ProofContext) -> ProofResult<PeerFixture> {
    let registry = super::delivery_matrix_support::selected_peer_registry(&proof.root)?;
    PeerFixture::start_in_registry(&registry, &proof.endpoint).await
}
