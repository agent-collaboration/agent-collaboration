//! An opt-in fixture ACP provider for end-to-end journeys that need a recipient the Router
//! can hold mail for and later load: a Cursor-endpoint session whose provider is a test
//! fixture. Absent unless the directory holds `fixture-provider-acp.json`.
use codex_router_host::{ExternalProviderLaunchBinding, ExternalProviderStartup};
use std::{error::Error, path::Path};

/// The launch a journey writes before it starts the Host.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureProviderLaunch {
    executable: std::path::PathBuf,
    arguments: Vec<String>,
}

pub fn startups(directory: &Path) -> Result<Vec<ExternalProviderStartup>, Box<dyn Error>> {
    let path = directory.join("fixture-provider-acp.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let launch: FixtureProviderLaunch = serde_json::from_slice(&std::fs::read(path)?)?;
    let binding = ExternalProviderLaunchBinding::cursor(launch.executable, launch.arguments)?;
    Ok(vec![ExternalProviderStartup::Launch(binding)])
}
