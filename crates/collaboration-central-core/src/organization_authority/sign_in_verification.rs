//! The Fleet object's injected checks against the sign-in service's keys: its
//! organization stamp, and a person's DPoP-bound admin request.
use super::{
    FleetOrganizationStatementRefusal, FleetOrganizationStatementVerifier,
    VerifiedFleetOrganizationStatement,
};
use crate::person_request::{
    Es256Verification, PersonRequestInput, PersonRequestRefusal, PersonRequestVerifier,
    VerifiedPersonRequest,
};
use crate::{Ed25519Verification, IssuerKeySource};
use durable_steps::BoxFuture;

/// Hosts build one per Fleet object, bound to that object's organization, the
/// sign-in issuer and audiences, its published keys and the host's crypto.
pub trait SignInVerification: Send + Sync + 'static {
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>;

    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>>;

    /// The sign-in service's own ServiceId: the identity of the link authority
    /// and the installation recorded on what it writes for a person. None when
    /// the deployment configured none; admin calls then refuse.
    fn sign_in_service(&self) -> Option<message_board::ServiceId>;
}

/// The two core verifiers, sharing one key source configuration and crypto.
pub struct SignInVerifiers<Ed, Es, StatementKeys, PersonKeys> {
    pub statements: FleetOrganizationStatementVerifier<Ed, StatementKeys>,
    pub persons: PersonRequestVerifier<Ed, Es, PersonKeys>,
    pub sign_in_service: Option<message_board::ServiceId>,
}

impl<Ed, Es, StatementKeys, PersonKeys> SignInVerification
    for SignInVerifiers<Ed, Es, StatementKeys, PersonKeys>
where
    Ed: Ed25519Verification,
    Es: Es256Verification,
    StatementKeys: IssuerKeySource + 'static,
    PersonKeys: IssuerKeySource + 'static,
{
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>
    {
        Box::pin(self.statements.verify(statement))
    }

    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>> {
        Box::pin(self.persons.verify(input))
    }

    fn sign_in_service(&self) -> Option<message_board::ServiceId> {
        self.sign_in_service.clone()
    }
}
