//! Read-only human-owned machine registry with strict JSONC and domain validation.
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

#[path = "router_connections/connection_profile.rs"]
mod connection_profile;
use connection_profile::RouterRegistryDocument;
pub use connection_profile::{
    AbsoluteRemoteCwd, AbsoluteSourcePath, RouterConnectionProfile, RouterConnectionTransport,
    RouterRegistryError, SourceClientProfile, SourceSshConnection,
};

const MAX_REGISTRY_BYTES: u64 = 65536;
/// The session picker's machine list at the Router root.
pub const MACHINE_LIST_FILE_NAME: &str = "agent-sessions.jsonc";
/// The machine list's earlier name; `agent-router-update` renames it.
const LEGACY_MACHINE_LIST_FILE_NAME: &str = "routers.jsonc";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RouterConnectionRegistry {
    pub routers: Vec<RouterConnectionProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouterRegistryRead {
    Missing,
    Ready(RouterConnectionRegistry),
    Rejected(RouterRegistryError),
}

impl RouterConnectionRegistry {
    pub fn parse(text: &str) -> Result<Self, RouterRegistryError> {
        let options = jsonc_parser::ParseOptions {
            allow_comments: true,
            allow_trailing_commas: true,
            allow_loose_object_property_names: false,
            allow_missing_commas: false,
            allow_single_quoted_strings: false,
            allow_hexadecimal_numbers: false,
            allow_unary_plus_numbers: false,
            allow_bare_decimal_point_numbers: false,
            allow_non_finite_numbers: false,
            allow_extended_string_escapes: false,
        };
        let parsed = jsonc_parser::parse_to_ast(text, &Default::default(), &options)
            .map_err(|_| RouterRegistryError::InvalidJsonc)?;
        let value = parsed.value.ok_or(RouterRegistryError::InvalidShape)?;
        reject_duplicate_members(&value)?;
        let document: RouterRegistryDocument =
            serde_json::from_value(value.into()).map_err(|_| RouterRegistryError::InvalidShape)?;
        if document.version != 1 {
            return Err(RouterRegistryError::UnsupportedVersion);
        }
        let mut names = BTreeSet::new();
        let mut routers = Vec::with_capacity(document.routers.len());
        for profile in document.routers {
            let profile = RouterConnectionProfile::try_from(profile)?;
            if !names.insert(profile.name.clone()) {
                return Err(RouterRegistryError::DuplicateName);
            }
            routers.push(profile);
        }
        Ok(Self { routers })
    }
}

pub fn read_router_registry(root: &Path) -> RouterRegistryRead {
    // The old name is never read: alone it means the update has not run, and
    // beside the new name it is a conflict the owner resolves.
    match std::fs::symlink_metadata(root.join(LEGACY_MACHINE_LIST_FILE_NAME)) {
        Ok(_) => {
            return RouterRegistryRead::Rejected(
                match std::fs::symlink_metadata(root.join(MACHINE_LIST_FILE_NAME)) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        RouterRegistryError::LegacyMachineListOnly
                    }
                    Ok(_) => RouterRegistryError::BothMachineListsPresent,
                    Err(_) => RouterRegistryError::Unreadable,
                },
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return RouterRegistryRead::Rejected(RouterRegistryError::Unreadable),
    }
    let mut file = match File::open(root.join(MACHINE_LIST_FILE_NAME)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RouterRegistryRead::Missing;
        }
        Err(_) => return RouterRegistryRead::Rejected(RouterRegistryError::Unreadable),
    };
    let mut text = String::new();
    if file
        .by_ref()
        .take(MAX_REGISTRY_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
    {
        return RouterRegistryRead::Rejected(RouterRegistryError::Unreadable);
    }
    if text.len() as u64 > MAX_REGISTRY_BYTES {
        return RouterRegistryRead::Rejected(RouterRegistryError::TooLarge);
    }
    match RouterConnectionRegistry::parse(&text) {
        Ok(registry) => RouterRegistryRead::Ready(registry),
        Err(reason) => RouterRegistryRead::Rejected(reason),
    }
}

fn reject_duplicate_members<'a>(
    value: &'a jsonc_parser::ast::Value<'a>,
) -> Result<(), RouterRegistryError> {
    match value {
        jsonc_parser::ast::Value::Object(object) => {
            let mut names = BTreeSet::new();
            for property in &object.properties {
                if !names.insert(property.name.as_str()) {
                    return Err(RouterRegistryError::DuplicateMembers);
                }
                reject_duplicate_members(&property.value)?;
            }
        }
        jsonc_parser::ast::Value::Array(array) => {
            for element in &array.elements {
                reject_duplicate_members(element)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "router_connections/registry_tests.rs"]
mod tests;
