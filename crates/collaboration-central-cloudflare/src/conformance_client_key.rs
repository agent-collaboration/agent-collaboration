//! RFC8032 test1 is public test material, excluded with the conformance feature.
use crate::CloudflareEd25519ClientSigner;
use collaboration_central_core::{ClientAssertionRefusal, IssuerKeyId};

pub(crate) async fn public_fixture_signer()
-> Result<CloudflareEd25519ClientSigner, ClientAssertionRefusal> {
    let key_id = IssuerKeyId::try_from("rfc8032-test-1".to_owned())
        .map_err(|_| ClientAssertionRefusal::InvalidConfiguration)?;
    let secret =
        include_bytes!("../../collaboration-central-core/tests/fixtures/rfc8032-test1.pkcs8");
    CloudflareEd25519ClientSigner::from_pkcs8(key_id, secret).await
}
