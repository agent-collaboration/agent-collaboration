//! Quota owns its grouping choice and validates the opaque stored preference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum QuotaGroupFacet {
    #[default]
    None,
    Provider,
    Status,
}

impl QuotaGroupFacet {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Provider => "Provider",
            Self::Status => "Status",
        }
    }

    pub const fn next_facet(self) -> Self {
        match self {
            Self::None => Self::Provider,
            Self::Provider => Self::Status,
            Self::Status => Self::None,
        }
    }

    pub const fn stored_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Provider => "provider",
            Self::Status => "status",
        }
    }

    pub fn from_stored_value(value: Option<&str>) -> Self {
        match value {
            Some("provider") => Self::Provider,
            Some("status") => Self::Status,
            _ => Self::None,
        }
    }
}

#[cfg(test)]
#[path = "quota_grouping_test.rs"]
mod tests;
