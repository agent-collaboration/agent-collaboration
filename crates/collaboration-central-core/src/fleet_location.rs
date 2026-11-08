//! One explicit legacy organization owns the reserved physical Fleet name.
use fleet_identity_protocol::OrganizationId;

#[derive(Clone, Debug)]
pub struct FleetLocationPolicy {
    legacy_organization: OrganizationId,
}

impl FleetLocationPolicy {
    #[must_use]
    pub fn new(legacy_organization: OrganizationId) -> Self {
        Self {
            legacy_organization,
        }
    }

    #[must_use]
    pub fn legacy_organization(&self) -> &OrganizationId {
        &self.legacy_organization
    }

    #[must_use]
    pub fn cloudflare_name(&self, organization: &OrganizationId) -> String {
        if organization == &self.legacy_organization {
            "fleet".into()
        } else {
            organization.as_str().to_owned()
        }
    }

    #[must_use]
    pub fn native_key(&self, organization: &OrganizationId) -> String {
        if organization == &self.legacy_organization {
            "fleet".into()
        } else {
            format!("fleet-{}", organization.as_str())
        }
    }

    pub fn organization_from_native_key(
        &self,
        key: &str,
    ) -> Result<OrganizationId, FleetLocationRefusal> {
        if key == "fleet" {
            return Ok(self.legacy_organization.clone());
        }
        let organization = key
            .strip_prefix("fleet-")
            .ok_or(FleetLocationRefusal::InvalidPhysicalKey)?
            .to_owned()
            .try_into()
            .map_err(|_| FleetLocationRefusal::InvalidPhysicalKey)?;
        if organization == self.legacy_organization {
            return Err(FleetLocationRefusal::InvalidPhysicalKey);
        }
        Ok(organization)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FleetLocationRefusal {
    #[error("Fleet physical key is invalid or noncanonical")]
    InvalidPhysicalKey,
}

#[cfg(test)]
#[path = "fleet_location_test.rs"]
mod tests;
