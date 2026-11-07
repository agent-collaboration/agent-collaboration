//! Bounded person-token experiment, not an authorization/admission path.
//!
//! The host supplies trusted JWKS, a public endpoint and an applying clock.
//! No key fetch, replay reservation, permission/factor check or floor read occurs here.
mod jwt_validation;
mod proof_verification;
mod request_contract;

pub use proof_verification::verify_person_request;
pub use request_contract::{
    DpopCrypto, DpopRefusal, DpopVerification, Ed25519Jwk, Es256Jwk, PersonDpopInput,
    PersonDpopPolicy, PrototypeOperation, VerifiedPersonRequest,
};
