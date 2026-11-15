//! Per-session half of Router's permission profiles and the check of what native applied.
//!
//! Router launches its managed app-server with the network half of both profiles
//! (`router_profile_projection.rs` in codex-native-integration). Each start, fork and
//! resume adds the parent profile and filesystem grants here, with dotted keys so
//! they merge into that network table instead of replacing it. The tool locations
//! themselves live in `router_tool_locations.rs`, shared with the Host that creates them.
use super::SessionSetupError;
use codex_native_integration::{
    READ_ONLY_INSIDE_TOOL_LOCATIONS, RESTRICTED_TOOL_LOCATIONS, WORKSPACE_TOOL_LOCATIONS,
};
use collaboration_protocol::{PermissionDifference, RouterAccess};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const fn profile_name(access: RouterAccess) -> &'static str {
    match access {
        RouterAccess::WriteRestricted => "router-write-restricted",
        RouterAccess::WorkspaceWrite => "router-workspace-write",
    }
}

pub(super) const fn access_name(access: RouterAccess) -> &'static str {
    match access {
        RouterAccess::WriteRestricted => "write-restricted",
        RouterAccess::WorkspaceWrite => "workspace-write",
    }
}

const fn parent_profile(access: RouterAccess) -> &'static str {
    match access {
        RouterAccess::WriteRestricted => ":read-only",
        RouterAccess::WorkspaceWrite => ":workspace",
    }
}

/// The profile one Router session selects: its access and every root it may write
/// beyond what the parent profile already grants.
pub(super) struct RouterSessionProfile {
    access: RouterAccess,
    /// The session's scratch folder as native sees it; also its shell `TMPDIR`.
    scratch: String,
    writable_roots: BTreeSet<String>,
    read_only_exceptions: BTreeSet<String>,
}

/// Host locations a profile's grants are resolved against.
struct HostLocations {
    home: PathBuf,
    /// Canonical system temporary directories (`$TMPDIR`, `/tmp`), resolved only for
    /// `write-restricted`, whose `:read-only` parent grants none.
    temporary_directories: Vec<PathBuf>,
    /// Tool locations with a symlinked component below `home`. Codex refuses a whole
    /// sandbox that grants a symlinked writable root, so these are not granted.
    symlinked_locations: BTreeSet<&'static str>,
}

impl HostLocations {
    /// Reads the host's `HOME` and, for `write-restricted`, its temporary directories,
    /// as the managed app-server inherits both from this process. Writes nothing: the
    /// Host creates missing tool locations at startup.
    async fn resolve(access: RouterAccess) -> Result<Self, SessionSetupError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or_else(|| SessionSetupError::HostLocationsUnavailable {
                location: "HOME".to_owned(),
            })?;
        let mut temporary_directories = Vec::new();
        if access == RouterAccess::WriteRestricted {
            let candidates = std::env::var_os("TMPDIR")
                .map(PathBuf::from)
                .into_iter()
                .chain([PathBuf::from("/tmp")])
                .filter(|path| path.is_absolute());
            for candidate in candidates {
                // Seatbelt matches canonical paths, so `/tmp` is granted as `/private/tmp`.
                // An unresolvable directory fails setup rather than silently losing a grant.
                let canonical = tokio::fs::canonicalize(&candidate).await.map_err(|_| {
                    SessionSetupError::HostLocationsUnavailable {
                        location: candidate.to_string_lossy().into_owned(),
                    }
                })?;
                temporary_directories.push(canonical);
            }
        }
        let mut symlinked_locations = BTreeSet::new();
        for location in tool_locations(access) {
            if has_symlinked_component(&home, location).await {
                symlinked_locations.insert(*location);
            }
        }
        Ok(Self {
            home,
            temporary_directories,
            symlinked_locations,
        })
    }
}

/// `:workspace` grants native's special `:tmpdir` and `:slash_tmp` paths; a
/// workspace-write session reads them only, so the system `$TMPDIR` and `/tmp`
/// are not granted (R4). `:read-only` grants neither, and write-restricted
/// keeps its explicit temporary grants.
fn withheld_temporary_directories(access: RouterAccess) -> impl Iterator<Item = (String, Value)> {
    let withheld: &[&str] = match access {
        RouterAccess::WorkspaceWrite => &[":tmpdir", ":slash_tmp"],
        RouterAccess::WriteRestricted => &[],
    };
    withheld
        .iter()
        .map(|special| ((*special).to_owned(), json!("read")))
}

/// The tool locations an access mode may write.
fn tool_locations(access: RouterAccess) -> &'static [&'static str] {
    match access {
        RouterAccess::WorkspaceWrite => &WORKSPACE_TOOL_LOCATIONS,
        RouterAccess::WriteRestricted => &RESTRICTED_TOOL_LOCATIONS,
    }
}

/// Whether any existing component of `location` below `home` is a symlink.
async fn has_symlinked_component(home: &Path, location: &str) -> bool {
    let mut path = home.to_path_buf();
    for component in Path::new(location).components() {
        path.push(component);
        match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            // A missing component cannot be a symlink, nor can anything below it.
            Err(_) => return false,
        }
    }
    false
}

/// Native resolves macOS system aliases, but preserves nested logical symlinks.
/// Missing or unreadable recorded scratch paths keep their existing spelling.
#[cfg_attr(
    not(target_os = "macos"),
    expect(clippy::unused_async, reason = "awaits only on macOS")
)]
async fn scratch_root_for_native(scratch: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if ["/tmp", "/var", "/etc"]
        .iter()
        .any(|alias| scratch.starts_with(alias))
    {
        for ancestor in scratch.ancestors() {
            let Ok(metadata) = tokio::fs::symlink_metadata(ancestor).await else {
                return scratch.to_path_buf();
            };
            if metadata.file_type().is_symlink()
                && ancestor.parent().and_then(Path::parent).is_some()
            {
                return scratch.to_path_buf();
            }
        }
        if let Ok(canonical) = tokio::fs::canonicalize(scratch).await {
            return canonical;
        }
    }
    scratch.to_path_buf()
}

impl RouterSessionProfile {
    /// Resolves the profile for a session against the host's home and temporary directories.
    pub(super) async fn for_session(
        access: RouterAccess,
        cwd: &Path,
        scratch: &Path,
    ) -> Result<Self, SessionSetupError> {
        let host = HostLocations::resolve(access).await?;
        let scratch_root = scratch_root_for_native(scratch).await;
        Ok(Self::with_host(access, cwd, &scratch_root, &host))
    }

    fn with_host(access: RouterAccess, cwd: &Path, scratch: &Path, host: &HostLocations) -> Self {
        let mut writable_roots = BTreeSet::from([scratch.to_string_lossy().into_owned()]);
        writable_roots.extend(
            tool_locations(access)
                .iter()
                .filter(|location| !host.symlinked_locations.contains(*location))
                .map(|location| host.home.join(location).to_string_lossy().into_owned()),
        );
        if access == RouterAccess::WriteRestricted {
            writable_roots.insert(cwd.join("tmp").to_string_lossy().into_owned());
            writable_roots.insert(cwd.join("docs/wip").to_string_lossy().into_owned());
            // `:read-only` grants no temporary directory, yet git, pnpm and SwiftPM
            // write there. Explicit paths keep native's report exact, where the
            // `:tmpdir` token would come back as roots Router cannot predict.
            writable_roots.extend(
                host.temporary_directories
                    .iter()
                    .map(|directory| directory.to_string_lossy().into_owned()),
            );
        }
        let read_only_exceptions = READ_ONLY_INSIDE_TOOL_LOCATIONS
            .iter()
            .map(|location| host.home.join(location).to_string_lossy().into_owned())
            .collect();
        Self {
            access,
            scratch: scratch.to_string_lossy().into_owned(),
            writable_roots,
            read_only_exceptions,
        }
    }

    pub(super) const fn name(&self) -> &'static str {
        profile_name(self.access)
    }

    /// Native `config` entries selecting this profile on start, fork or resume.
    pub(super) fn native_config(&self) -> Map<String, Value> {
        let profile = self.name();
        let filesystem = self
            .writable_roots
            .iter()
            .map(|root| (root.clone(), json!("write")))
            .chain(
                self.read_only_exceptions
                    .iter()
                    .map(|path| (path.clone(), json!("read"))),
            )
            .chain(withheld_temporary_directories(self.access))
            .collect::<Map<_, _>>();
        let mut config = Map::from_iter([
            ("default_permissions".to_owned(), json!(profile)),
            (
                format!("permissions.{profile}.extends"),
                json!(parent_profile(self.access)),
            ),
            (
                format!("permissions.{profile}.filesystem"),
                Value::Object(filesystem),
            ),
        ]);
        if self.access == RouterAccess::WorkspaceWrite {
            // Temporary files of git, cargo or pnpm land in the session's own
            // folder instead of asking. One dotted key leaves the user's own
            // `set` entries in place.
            config.insert(
                "shell_environment_policy.set.TMPDIR".to_owned(),
                json!(self.scratch),
            );
        }
        config
    }

    /// Reports mismatched or unavailable permission fields without refusing setup.
    pub(super) fn observed_differences(&self, response: &Value) -> Vec<PermissionDifference> {
        let profile_id = response
            .pointer("/activePermissionProfile/id")
            .and_then(Value::as_str);
        let actual_roots = response
            .pointer("/sandbox/writableRoots")
            .and_then(Value::as_array)
            .and_then(|roots| {
                roots
                    .iter()
                    .map(|root| root.as_str().map(str::to_owned))
                    .collect::<Option<BTreeSet<_>>>()
            });
        let sandbox_type = response.pointer("/sandbox/type").and_then(Value::as_str);
        let network_access = response
            .pointer("/sandbox/networkAccess")
            .and_then(Value::as_bool);
        // R4: a workspace-write sandbox must exclude both system temporary
        // directories; native reports that it does through these two flags.
        let temporary_directories_withheld = self.access != RouterAccess::WorkspaceWrite
            || ["/sandbox/excludeTmpdirEnvVar", "/sandbox/excludeSlashTmp"]
                .iter()
                .all(|flag| response.pointer(flag).and_then(Value::as_bool) == Some(true));
        // Keep exact comparisons as evidence. Missing and malformed values differ too.
        // The invisible proxy state is not compared.
        [
            (
                profile_id != Some(self.name()),
                PermissionDifference::Profile,
            ),
            (
                sandbox_type != Some("workspaceWrite") || !temporary_directories_withheld,
                PermissionDifference::Sandbox,
            ),
            (
                actual_roots.as_ref() != Some(&self.writable_roots),
                PermissionDifference::WritableRoots,
            ),
            (network_access != Some(true), PermissionDifference::Network),
        ]
        .into_iter()
        .filter_map(|(differs, field)| differs.then_some(field))
        .collect()
    }
}

#[cfg(test)]
#[path = "router_permission_profile_test.rs"]
mod tests;
