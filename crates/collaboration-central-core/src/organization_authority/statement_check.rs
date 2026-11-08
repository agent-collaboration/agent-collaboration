//! The Fleet object's injected way to check a stamp against the sign-in keys.
use super::{
    FleetOrganizationStatementRefusal, FleetOrganizationStatementVerifier,
    VerifiedFleetOrganizationStatement,
};
use crate::{Ed25519Verification, IssuerKeySource};
use durable_steps::BoxFuture;

/// Hosts build one per Fleet object, bound to that object's organization, the
/// sign-in issuer and audience, its published keys and the host's crypto.
pub trait OrganizationStatementCheck: Send + Sync + 'static {
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>;
}

impl<Crypto, Keys> OrganizationStatementCheck for FleetOrganizationStatementVerifier<Crypto, Keys>
where
    Crypto: Ed25519Verification,
    Keys: IssuerKeySource + 'static,
{
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>
    {
        Box::pin(self.verify(statement))
    }
}
