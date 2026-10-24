//! Fleet secrets belong to one installation and one independently published enrollment generation.
use crate::model::{SecretKey, SecretStoreError};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FleetSecretKind {
    AccessClientSecret,
    MachineCredential,
}
impl FleetSecretKind {
    const fn prefix(self) -> &'static str {
        match self {
            Self::AccessClientSecret => "fleet_access_client_secret",
            Self::MachineCredential => "fleet_machine_credential",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FleetSecretKey {
    kind: FleetSecretKind,
    installation: Uuid,
    generation: Uuid,
}
impl FleetSecretKey {
    pub fn new(
        kind: FleetSecretKind,
        installation: &str,
        generation: &str,
    ) -> Result<Self, SecretStoreError> {
        Ok(Self {
            kind,
            installation: canonical_uuid(installation)?,
            generation: canonical_uuid(generation)?,
        })
    }
    /// The entire Fleet namespace is reserved, so malformed Fleet keys cannot select plaintext.
    pub fn parse(key: &SecretKey) -> Result<Option<Self>, SecretStoreError> {
        if !key.as_str().starts_with("fleet_") {
            return Ok(None);
        }
        let mut parts = key.as_str().split('.');
        let kind = match parts.next() {
            Some("fleet_access_client_secret") => FleetSecretKind::AccessClientSecret,
            Some("fleet_machine_credential") => FleetSecretKind::MachineCredential,
            _ => return Err(SecretStoreError::InvalidFleetSecretKey),
        };
        let installation = parts
            .next()
            .ok_or(SecretStoreError::InvalidFleetSecretKey)?;
        let generation = parts
            .next()
            .ok_or(SecretStoreError::InvalidFleetSecretKey)?;
        if parts.next().is_some() {
            return Err(SecretStoreError::InvalidFleetSecretKey);
        }
        Self::new(kind, installation, generation).map(Some)
    }
    pub fn secret_key(&self) -> Result<SecretKey, SecretStoreError> {
        SecretKey::new(format!(
            "{}.{}.{}",
            self.kind.prefix(),
            self.installation,
            self.generation
        ))
    }
    #[must_use]
    pub const fn kind(&self) -> FleetSecretKind {
        self.kind
    }
    #[must_use]
    pub fn installation(&self) -> String {
        self.installation.to_string()
    }
    #[must_use]
    pub fn generation(&self) -> String {
        self.generation.to_string()
    }
}

fn canonical_uuid(value: &str) -> Result<Uuid, SecretStoreError> {
    let parsed = Uuid::parse_str(value).map_err(|_| SecretStoreError::InvalidFleetSecretKey)?;
    if parsed.is_nil() || parsed.to_string() != value {
        return Err(SecretStoreError::InvalidFleetSecretKey);
    }
    Ok(parsed)
}
