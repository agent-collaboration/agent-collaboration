//! An owner identity exists only after the shared Access verifier accepts it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OwnerIdentity(String);
impl OwnerIdentity {
    pub fn from_access(caller: crate::AccessCaller) -> Result<Self, crate::AccessRefusal> {
        match caller {
            crate::AccessCaller::Owner { email } => Ok(Self(email)),
            crate::AccessCaller::Machine { .. } => Err(crate::AccessRefusal::WrongCallerClass),
        }
    }
}
