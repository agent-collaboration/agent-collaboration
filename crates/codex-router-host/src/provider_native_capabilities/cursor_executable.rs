//! Source identity for the exact native Cursor launch, without changing it.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

// Exact inspected 2026.10.22-27dcf5f distribution entry point; not a shell interpreter.
const QUALIFIED_VERSIONED_LAUNCHER: &[u8] = br#"#!/usr/bin/env bash
set -euo pipefail
export CURSOR_INVOKED_AS="$(basename "$0")"
# Get the directory of the actual script (handles symlinks)
if command -v realpath >/dev/null 2>&1; then
  SCRIPT_DIR="$(dirname "$(realpath "$0")")"
else
  SCRIPT_DIR="$(dirname "$(readlink "$0" || echo "$0")")"
fi
NODE_BIN="$SCRIPT_DIR/node"

# Enable Node.js compile cache for faster CLI startup (requires Node.js >= 22.1.0)
# Cache is automatically invalidated when source files change
if [ -z "${NODE_COMPILE_CACHE:-}" ] && [ -n "${HOME:-}" ]; then
  if [[ "${OSTYPE:-}" == darwin* ]]; then
    export NODE_COMPILE_CACHE="$HOME/Library/Caches/cursor-compile-cache"
  else
    export NODE_COMPILE_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/cursor-compile-cache"
  fi
fi

should_skip_system_ca() {
  case "${AGENT_CLI_CREDENTIAL_STORE:-}" in
    file)
      return 0
      ;;
  esac
  return 1
}

if ! should_skip_system_ca && "$NODE_BIN" --use-system-ca --version >/dev/null 2>&1; then
  exec -a "$0" "$NODE_BIN" --use-system-ca "$SCRIPT_DIR/index.js" "$@"
fi

exec -a "$0" "$NODE_BIN" "$SCRIPT_DIR/index.js" "$@"
"#;

fn qualified_versioned_launcher(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    let Ok(maximum) = u64::try_from(QUALIFIED_VERSIONED_LAUNCHER.len()) else {
        return false;
    };
    if file.take(maximum + 1).read_to_end(&mut bytes).is_err() {
        return false;
    }
    bytes == QUALIFIED_VERSIONED_LAUNCHER
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceFileIdentity {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn source_identity(path: &Path) -> Option<SourceFileIdentity> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Some(SourceFileIdentity {
        length: metadata.len(),
        modified: metadata.modified().ok()?,
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    })
}

#[derive(Clone, Debug)]
pub(super) struct CursorExecutableSource {
    configured_path: PathBuf,
    physical_launcher: PathBuf,
    sources: Vec<(PathBuf, SourceFileIdentity)>,
    confirmed_after_launch: bool,
}

impl CursorExecutableSource {
    pub(super) fn capture(configured_path: &Path) -> Option<Self> {
        if !configured_path.is_absolute() {
            tracing::warn!(
                phase = "cursor-source-capture",
                category = "relative-launcher",
                "native catalogue source qualification failed"
            );
            return None;
        }
        let physical_launcher = fs::canonicalize(configured_path).ok().or_else(|| {
            tracing::warn!(
                phase = "cursor-source-capture",
                category = "launcher-unresolved",
                "native catalogue source qualification failed"
            );
            None
        })?;
        if !qualified_versioned_launcher(&physical_launcher) {
            tracing::warn!(
                phase = "cursor-source-capture",
                category = "launcher-form-unqualified",
                "native catalogue source qualification failed"
            );
            return None;
        }
        let directory = physical_launcher.parent()?;
        let node = directory.join("node");
        let mut header = [0; 4];
        fs::File::open(&node)
            .and_then(|mut file| file.read_exact(&mut header))
            .ok()
            .or_else(|| {
                tracing::warn!(
                    phase = "cursor-source-capture",
                    category = "node-unreadable",
                    "native catalogue source qualification failed"
                );
                None
            })?;
        // The inspected launcher fixes both node and index to its physical
        // directory. Only that adjacent native node image is qualified here.
        if !matches!(
            header,
            [0xcf, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
        ) {
            tracing::warn!(
                phase = "cursor-source-capture",
                category = "node-form-unqualified",
                "native catalogue source qualification failed"
            );
            return None;
        }
        let paths = [
            physical_launcher.clone(),
            node,
            directory.join("index.js"),
            directory.join("7096.index.js"),
            directory.join("5622.index.js"),
        ];
        let sources = paths
            .into_iter()
            .enumerate()
            .map(|(index, path)| {
                let identity = source_identity(&path).or_else(|| {
                    let category = match index {
                        0 => "launcher-identity-unavailable",
                        1 => "node-identity-unavailable",
                        2 => "index-identity-unavailable",
                        3 => "acp-payload-identity-unavailable",
                        _ => "loader-payload-identity-unavailable",
                    };
                    tracing::warn!(
                        phase = "cursor-source-capture",
                        category,
                        "native catalogue source qualification failed"
                    );
                    None
                })?;
                Some((path, identity))
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            configured_path: configured_path.to_owned(),
            physical_launcher,
            sources,
            confirmed_after_launch: false,
        })
    }

    fn unchanged(&self) -> bool {
        if fs::canonicalize(&self.configured_path).ok().as_ref() != Some(&self.physical_launcher) {
            tracing::warn!(
                phase = "cursor-source-freshness",
                category = "launcher-retargeted-or-unavailable",
                "native catalogue source qualification failed"
            );
            return false;
        }
        if !qualified_versioned_launcher(&self.physical_launcher) {
            tracing::warn!(
                phase = "cursor-source-freshness",
                category = "launcher-form-changed",
                "native catalogue source qualification failed"
            );
            return false;
        }
        self.sources
            .iter()
            .enumerate()
            .all(|(index, (path, identity))| {
                let unchanged = source_identity(path).as_ref() == Some(identity);
                if !unchanged {
                    let category = match index {
                        0 => "launcher-identity-changed",
                        1 => "node-identity-changed",
                        2 => "index-identity-changed",
                        3 => "acp-payload-identity-changed",
                        _ => "loader-payload-identity-changed",
                    };
                    tracing::warn!(
                        phase = "cursor-source-freshness",
                        category,
                        "native catalogue source qualification failed"
                    );
                }
                unchanged
            })
    }

    pub(super) fn confirm_after_launch(&mut self) {
        self.confirmed_after_launch = self.unchanged();
        tracing::info!(
            phase = "cursor-source-confirmation",
            qualified = self.confirmed_after_launch,
            "native catalogue launch source checked"
        );
    }

    pub(super) fn executable_for_read(&self) -> Option<&Path> {
        // Preserve the bound launcher's $0/CURSOR_INVOKED_AS while its physical
        // source and every adjacent input still match the admitted identity.
        if !self.confirmed_after_launch {
            tracing::warn!(
                phase = "cursor-source-read",
                category = "launch-source-not-confirmed",
                "native catalogue source qualification failed"
            );
            return None;
        }
        self.unchanged().then_some(self.configured_path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Literal fixture copied from the independently inspected 2026.10.22 launcher.
    const VERSIONED_LAUNCHER_FIXTURE: &[u8] = br#"#!/usr/bin/env bash
set -euo pipefail
export CURSOR_INVOKED_AS="$(basename "$0")"
# Get the directory of the actual script (handles symlinks)
if command -v realpath >/dev/null 2>&1; then
  SCRIPT_DIR="$(dirname "$(realpath "$0")")"
else
  SCRIPT_DIR="$(dirname "$(readlink "$0" || echo "$0")")"
fi
NODE_BIN="$SCRIPT_DIR/node"

# Enable Node.js compile cache for faster CLI startup (requires Node.js >= 22.1.0)
# Cache is automatically invalidated when source files change
if [ -z "${NODE_COMPILE_CACHE:-}" ] && [ -n "${HOME:-}" ]; then
  if [[ "${OSTYPE:-}" == darwin* ]]; then
    export NODE_COMPILE_CACHE="$HOME/Library/Caches/cursor-compile-cache"
  else
    export NODE_COMPILE_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/cursor-compile-cache"
  fi
fi

should_skip_system_ca() {
  case "${AGENT_CLI_CREDENTIAL_STORE:-}" in
    file)
      return 0
      ;;
  esac
  return 1
}

if ! should_skip_system_ca && "$NODE_BIN" --use-system-ca --version >/dev/null 2>&1; then
  exec -a "$0" "$NODE_BIN" --use-system-ca "$SCRIPT_DIR/index.js" "$@"
fi

exec -a "$0" "$NODE_BIN" "$SCRIPT_DIR/index.js" "$@"
"#;

    fn write_versioned_distribution(directory: &Path) {
        fs::create_dir_all(directory).expect("version directory");
        fs::write(directory.join("cursor-agent"), VERSIONED_LAUNCHER_FIXTURE)
            .expect("versioned launcher");
        fs::write(directory.join("node"), [0xcf, 0xfa, 0xed, 0xfe]).expect("node identity fixture");
        for file in ["index.js", "7096.index.js", "5622.index.js"] {
            fs::write(directory.join(file), "native source fixture").expect("source fixture");
        }
    }

    #[test]
    fn native_capabilities_cursor_versioned_launcher_binds_adjacent_node_and_payloads() {
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        write_versioned_distribution(root.path());
        let launcher = root.path().join("cursor-agent");
        let mut source =
            CursorExecutableSource::capture(&launcher).expect("actual versioned distribution form");
        assert!(source.executable_for_read().is_none());
        source.confirm_after_launch();
        assert_eq!(source.executable_for_read(), Some(launcher.as_path()));
    }

    #[cfg(unix)]
    #[test]
    fn native_capabilities_cursor_source_retarget_or_payload_change_cannot_requalify_running_binding()
     {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        let first = root.path().join("first");
        let second = root.path().join("second");
        for directory in [&first, &second] {
            write_versioned_distribution(directory);
        }
        let launcher = root.path().join("agent");
        symlink(first.join("cursor-agent"), &launcher).expect("launcher");
        let mut source = CursorExecutableSource::capture(&launcher).expect("source identity");
        assert!(source.executable_for_read().is_none());
        source.confirm_after_launch();
        assert_eq!(source.executable_for_read(), Some(launcher.as_path()));
        fs::remove_file(&launcher).expect("owned launcher symlink");
        symlink(second.join("cursor-agent"), &launcher).expect("retargeted launcher");
        assert!(source.executable_for_read().is_none());
        fs::remove_file(&launcher).expect("owned launcher symlink");
        symlink(first.join("cursor-agent"), &launcher).expect("original launcher");
        fs::write(first.join("7096.index.js"), "changed ACP payload fixture")
            .expect("payload change");
        assert!(source.executable_for_read().is_none());
    }

    #[test]
    fn native_capabilities_cursor_loader_chunk_change_invalidates_running_binding() {
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        write_versioned_distribution(root.path());
        let mut source = CursorExecutableSource::capture(&root.path().join("cursor-agent"))
            .expect("actual source set");
        source.confirm_after_launch();
        assert!(source.executable_for_read().is_some());
        fs::write(
            root.path().join("5622.index.js"),
            "changed actual loader and skill reference parser",
        )
        .expect("loader change");
        assert!(
            source.executable_for_read().is_none(),
            "the actual loaded native skill parser changed"
        );
    }

    #[test]
    fn native_capabilities_cursor_versioned_launcher_rejects_other_scripts_and_missing_node() {
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        write_versioned_distribution(root.path());
        let launcher = root.path().join("cursor-agent");
        for contents in [
            b"#!/bin/sh\nexec node index.js \"$@\"\n".as_slice(),
            &[0xcf, 0xfa, 0xed, 0xfe],
        ] {
            fs::write(&launcher, contents).expect("unqualified entry point");
            assert!(CursorExecutableSource::capture(&launcher).is_none());
        }
        fs::write(&launcher, VERSIONED_LAUNCHER_FIXTURE).expect("restore owned fixture");
        fs::remove_file(root.path().join("node")).expect("remove owned node fixture");
        assert!(CursorExecutableSource::capture(&launcher).is_none());
        fs::write(root.path().join("node"), "#!/bin/sh\nexit 0\n").expect("unqualified node");
        assert!(CursorExecutableSource::capture(&launcher).is_none());
    }

    #[test]
    fn native_capabilities_cursor_each_bound_launcher_input_invalidates_after_change() {
        for file in [
            "cursor-agent",
            "node",
            "index.js",
            "7096.index.js",
            "5622.index.js",
        ] {
            let root = tempfile::tempdir_in("/tmp").expect("fixture");
            write_versioned_distribution(root.path());
            let launcher = root.path().join("cursor-agent");
            let mut source = CursorExecutableSource::capture(&launcher).expect("qualified form");
            source.confirm_after_launch();
            assert!(source.executable_for_read().is_some());
            fs::write(root.path().join(file), "changed bound source").expect("owned source change");
            assert!(source.executable_for_read().is_none(), "changed {file}");
            source.confirm_after_launch();
            assert!(
                source.executable_for_read().is_none(),
                "cannot requalify changed {file}"
            );
        }
    }
}
