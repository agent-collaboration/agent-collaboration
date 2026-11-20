//! An organization's injected checks against the sign-in service's keys: its
//! organization stamp, a person's DPoP-bound admin request and machine access tokens.
use super::{
    FleetOrganizationStatementVerifier, StoredStatementFacts, VerifiedFleetOrganizationStatement,
};
use crate::machine_token::{MachineTokenRefusal, MachineTokenVerifier, VerifiedMachineToken};
use crate::person_request::{
    PersonRequestInput, PersonRequestRefusal, PersonRequestVerifier, VerifiedPersonRequest,
};
use fleet_contracts::FleetOrganizationStatementRefusal;
use fleet_runtime_ports::Es256Verification;
use fleet_runtime_ports::{Ed25519Verification, IssuerKeySource};
use futures_util::future::BoxFuture;

/// Hosts build one per organization runtime, bound to that organization, the
/// sign-in issuer and audiences, its published keys and the host's crypto.
pub trait SignInVerification: Send + Sync + 'static {
    fn verify_statement<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedFleetOrganizationStatement, FleetOrganizationStatementRefusal>>;

    /// Verify the stored signature and profile for a facts-only digest read.
    /// This skips both current-time bounds and must never grant Valid standing.
    fn verify_stored_statement_facts<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<StoredStatementFacts, FleetOrganizationStatementRefusal>>;

    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>>;

    fn verify_machine_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedMachineToken, MachineTokenRefusal>> {
        let _ = token;
        Box::pin(async { Err(MachineTokenRefusal::Unavailable) })
    }

    /// Origin recorded for writes made by the sign-in authority for a person.
    /// Admin calls require this origin to be configured.
    fn sign_in_service(&self) -> Option<message_board::ServiceId>;
}

/// The three core verifiers share the configured issuer keys and host crypto.
pub struct SignInVerifiers<Ed, Es, StatementKeys, PersonKeys> {
    pub statements: FleetOrganizationStatementVerifier<Ed, StatementKeys>,
    pub persons: PersonRequestVerifier<Ed, Es, PersonKeys>,
    pub machines: MachineTokenVerifier<Ed, PersonKeys>,
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

    fn verify_stored_statement_facts<'a>(
        &'a self,
        statement: &'a str,
    ) -> BoxFuture<'a, Result<StoredStatementFacts, FleetOrganizationStatementRefusal>> {
        Box::pin(self.statements.verify_stored_statement_facts(statement))
    }

    fn verify_person_request<'a>(
        &'a self,
        input: &'a PersonRequestInput,
    ) -> BoxFuture<'a, Result<VerifiedPersonRequest, PersonRequestRefusal>> {
        Box::pin(self.persons.verify(input))
    }

    fn verify_machine_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<VerifiedMachineToken, MachineTokenRefusal>> {
        Box::pin(self.machines.verify(token))
    }

    fn sign_in_service(&self) -> Option<message_board::ServiceId> {
        self.sign_in_service.clone()
    }
}
