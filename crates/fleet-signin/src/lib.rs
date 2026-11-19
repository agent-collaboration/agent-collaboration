//! Verification and provisioning policy for organization sign-in.
#![forbid(unsafe_code)]

pub mod authorization_floor_rows;
mod authorization_revision_plans;
mod central_organization_identity;
pub mod identity_contract;
mod issuer_key_cache;
mod issuer_key_resolution;
mod issuer_signature_verifier;
pub mod organization_authority;
mod organization_authority_cache;
pub mod organization_change_rows;
mod organization_provisioning_plans;
mod organization_signin_runtime;
mod organization_stamp_rows;
pub mod person_request;
mod sign_in_trust_settings;

pub use central_organization_identity::{
    CentralOrganizationIdentity, prepare_central_alarm, read_central_organization,
};
pub use issuer_key_cache::{IssuerKeyCache, IssuerKeyId, IssuerKeySet, IssuerVerificationKey};
pub use issuer_key_resolution::resolve_issuer_key;
pub use issuer_signature_verifier::IssuerSignatureVerifier;
pub use organization_authority_cache::{OrganizationAuthority, StampStanding};
pub use organization_signin_runtime::OrganizationSignIn;
pub use sign_in_trust_settings::SignInTrustSettings;
