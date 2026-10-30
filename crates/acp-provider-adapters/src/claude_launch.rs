//! Qualify the existing official Claude installation before adapting its launch.

use crate::ClaudeSkillSource;
use acp_client_runtime::ExternalProviderLaunch;
use serde::Deserialize;
use session_event_model::SkillSourceError;
use std::time::Duration;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use tokio::process::Command;

const METADATA_MODULE: &str = include_str!("claude_metadata.mjs");

pub struct PreparedClaudeLaunch {
    pub launch: ExternalProviderLaunch,
    pub skill_source: ClaudeSkillSource,
    pub qualification: Result<ClaudeLaunchIdentity, ClaudeLaunchUnavailable>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeLaunchIdentity {
    pub library: PathBuf,
    pub sdk: PathBuf,
    pub node: PathBuf,
    pub node_version: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaudeLaunchUnavailable {
    UnsupportedArguments,
    ExecutableUnavailable,
    UnrecognizedShim,
    UnqualifiedPackage,
    NodeUnavailable,
    PublicApiUnavailable,
}

#[derive(Deserialize)]
struct PackageManifest {
    name: String,
    version: String,
    bin: Option<serde_json::Value>,
    exports: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct QualifiedImports {
    sdk: PathBuf,
    node_version: String,
}

pub async fn prepare_claude_launch(mut launch: ExternalProviderLaunch) -> PreparedClaudeLaunch {
    let qualification = qualify_launch(&launch).await.and_then(|identity| {
        let library_url = file_url(&identity.library)?;
        let sdk_url = file_url(&identity.sdk)?;
        Ok((identity, library_url, sdk_url))
    });
    match qualification {
        Ok((identity, library_url, sdk_url)) => {
            launch.executable = identity.node.clone();
            launch.arguments = vec![
                "--input-type=module".into(),
                "--eval".into(),
                METADATA_MODULE.into(),
                "--".into(),
                library_url,
                sdk_url,
            ];
            PreparedClaudeLaunch {
                launch,
                skill_source: ClaudeSkillSource::Annotated,
                qualification: Ok(identity),
            }
        }
        Err(reason) => PreparedClaudeLaunch {
            launch,
            skill_source: ClaudeSkillSource::Unavailable(SkillSourceError::UnqualifiedLaunch),
            qualification: Err(reason),
        },
    }
}

fn file_url(path: &Path) -> Result<String, ClaudeLaunchUnavailable> {
    // Both paths have been canonicalized; URL conversion preserves spaces and Unicode.
    url::Url::from_file_path(path)
        .map(Into::into)
        .map_err(|()| ClaudeLaunchUnavailable::UnqualifiedPackage)
}

async fn qualify_launch(
    launch: &ExternalProviderLaunch,
) -> Result<ClaudeLaunchIdentity, ClaudeLaunchUnavailable> {
    if !launch.arguments.is_empty() {
        return Err(ClaudeLaunchUnavailable::UnsupportedArguments);
    }
    let path = launch
        .environment
        .iter()
        .rev()
        .find(|(name, _)| name == "PATH")
        .map_or_else(
            || std::env::var_os("PATH").unwrap_or_default(),
            |(_, value)| OsString::from(value),
        );
    let configured = resolve_executable(&launch.executable, &path)
        .ok_or(ClaudeLaunchUnavailable::ExecutableUnavailable)?;
    let entry = official_entry(&configured)?;
    let package = entry
        .parent()
        .and_then(Path::parent)
        .ok_or(ClaudeLaunchUnavailable::UnqualifiedPackage)?;
    let manifest: PackageManifest = serde_json::from_slice(
        &std::fs::read(package.join("package.json"))
            .map_err(|_| ClaudeLaunchUnavailable::UnqualifiedPackage)?,
    )
    .map_err(|_| ClaudeLaunchUnavailable::UnqualifiedPackage)?;
    if manifest.name != "@agentclientprotocol/claude-agent-acp"
        || manifest.version != "0.81.0"
        || manifest
            .bin
            .as_ref()
            .and_then(|bin| bin.get("claude-agent-acp"))
            .and_then(serde_json::Value::as_str)
            != Some("dist/index.js")
        || manifest
            .exports
            .as_ref()
            .and_then(|exports| exports.pointer("/./import"))
            .and_then(serde_json::Value::as_str)
            != Some("./dist/lib.js")
        || std::fs::canonicalize(package.join("dist/index.js"))
            .ok()
            .as_ref()
            != Some(&entry)
    {
        return Err(ClaudeLaunchUnavailable::UnqualifiedPackage);
    }
    let library = std::fs::canonicalize(package.join("dist/lib.js"))
        .map_err(|_| ClaudeLaunchUnavailable::UnqualifiedPackage)?;
    let node = resolve_executable(Path::new("node"), &path)
        .ok_or(ClaudeLaunchUnavailable::NodeUnavailable)?;
    let mut command = Command::new(&node);
    command
        .args(["--input-type=module", "--eval", QUALIFY_IMPORTS, "--"])
        .arg(&library)
        .envs(launch.environment.iter().map(|(key, value)| (key, value)))
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .map_err(|_| ClaudeLaunchUnavailable::PublicApiUnavailable)?
        .map_err(|_| ClaudeLaunchUnavailable::NodeUnavailable)?;
    if !output.status.success() {
        return Err(ClaudeLaunchUnavailable::PublicApiUnavailable);
    }
    let imports: QualifiedImports = serde_json::from_slice(&output.stdout)
        .map_err(|_| ClaudeLaunchUnavailable::PublicApiUnavailable)?;
    Ok(ClaudeLaunchIdentity {
        library,
        sdk: imports.sdk,
        node,
        node_version: imports.node_version,
    })
}

fn resolve_executable(executable: &Path, path: &std::ffi::OsStr) -> Option<PathBuf> {
    if executable.components().count() > 1 {
        return std::fs::canonicalize(executable)
            .ok()
            .filter(|path| path.is_file());
    }
    std::env::split_paths(path).find_map(|directory| {
        std::fs::canonicalize(directory.join(executable))
            .ok()
            .filter(|path| path.is_file())
    })
}

fn official_entry(executable: &Path) -> Result<PathBuf, ClaudeLaunchUnavailable> {
    if executable
        .file_name()
        .is_some_and(|name| name == "index.js")
    {
        return Ok(executable.to_path_buf());
    }
    let shim = std::fs::read_to_string(executable)
        .map_err(|_| ClaudeLaunchUnavailable::UnrecognizedShim)?;
    let targets = shim
        .lines()
        .filter_map(|line| line.strip_prefix("# cmd-shim-target="))
        .collect::<Vec<_>>();
    let [target] = targets.as_slice() else {
        return Err(ClaudeLaunchUnavailable::UnrecognizedShim);
    };
    if !Path::new(target).is_absolute() {
        return Err(ClaudeLaunchUnavailable::UnrecognizedShim);
    }
    std::fs::canonicalize(target).map_err(|_| ClaudeLaunchUnavailable::UnqualifiedPackage)
}

const QUALIFY_IMPORTS: &str = r#"
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
const library = fs.realpathSync(process.argv[1]);
const sdk = fs.realpathSync(createRequire(pathToFileURL(library)).resolve('@anthropic-ai/claude-agent-sdk'));
let directory = path.dirname(sdk);
let qualified = false;
while (directory !== path.dirname(directory)) {
  const manifest = path.join(directory, 'package.json');
  if (fs.existsSync(manifest)) {
    const value = JSON.parse(fs.readFileSync(manifest, 'utf8'));
    if (value.name === '@anthropic-ai/claude-agent-sdk') { qualified = value.version === '0.3.280'; break; }
  }
  directory = path.dirname(directory);
}
if (!qualified || Number(process.versions.node.split('.')[0]) < 22) process.exit(2);
const official = await import(pathToFileURL(library).href);
const agentSdk = await import(pathToFileURL(sdk).href);
if (typeof official.runAcp !== 'function' || typeof agentSdk.resolveSettings !== 'function') process.exit(3);
process.stdout.write(JSON.stringify({sdk, node_version:process.versions.node}));
"#;
