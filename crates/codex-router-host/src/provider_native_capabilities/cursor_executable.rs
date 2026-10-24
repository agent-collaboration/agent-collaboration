//! Source identity for the exact native Cursor launch, without changing it.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

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
    actual_executable: PathBuf,
    sources: Vec<(PathBuf, SourceFileIdentity)>,
    confirmed_after_launch: bool,
}

impl CursorExecutableSource {
    pub(super) fn capture(configured_path: &Path) -> Option<Self> {
        if !configured_path.is_absolute() {
            return None;
        }
        let actual_executable = fs::canonicalize(configured_path).ok()?;
        let mut header = [0; 4];
        fs::File::open(&actual_executable)
            .ok()?
            .read_exact(&mut header)
            .ok()?;
        // This qualified distribution is the direct macOS native executable,
        // not a script that might choose another mutable executable internally.
        if !matches!(
            header,
            [0xcf, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
        ) {
            return None;
        }
        let directory = actual_executable.parent()?;
        let paths = [
            actual_executable.clone(),
            directory.join("index.js"),
            directory.join("7096.index.js"),
            directory.join("5622.index.js"),
        ];
        let sources = paths
            .into_iter()
            .map(|path| Some((path.clone(), source_identity(&path)?)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            configured_path: configured_path.to_owned(),
            actual_executable,
            sources,
            confirmed_after_launch: false,
        })
    }

    fn unchanged(&self) -> bool {
        fs::canonicalize(&self.configured_path).ok().as_ref() == Some(&self.actual_executable)
            && self
                .sources
                .iter()
                .all(|(path, identity)| source_identity(path).as_ref() == Some(identity))
    }

    pub(super) fn confirm_after_launch(&mut self) {
        self.confirmed_after_launch = self.unchanged();
    }

    pub(super) fn executable_for_read(&self) -> Option<&Path> {
        (self.confirmed_after_launch && self.unchanged())
            .then_some(self.actual_executable.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn native_capabilities_cursor_source_retarget_or_payload_change_cannot_requalify_running_binding()
     {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir_in("/tmp").expect("fixture");
        let first = root.path().join("first");
        let second = root.path().join("second");
        for directory in [&first, &second] {
            fs::create_dir(directory).expect("version directory");
            fs::write(directory.join("cursor-agent"), [0xcf, 0xfa, 0xed, 0xfe])
                .expect("source identity fixture");
            fs::write(directory.join("index.js"), "native source fixture").expect("index fixture");
            fs::write(directory.join("7096.index.js"), "ACP source fixture").expect("ACP fixture");
            fs::write(
                directory.join("5622.index.js"),
                "native loader source fixture",
            )
            .expect("loader fixture");
        }
        let launcher = root.path().join("agent");
        symlink(first.join("cursor-agent"), &launcher).expect("launcher");
        let mut source = CursorExecutableSource::capture(&launcher).expect("source identity");
        assert!(source.executable_for_read().is_none());
        source.confirm_after_launch();
        assert_eq!(
            source.executable_for_read(),
            Some(
                fs::canonicalize(first.join("cursor-agent"))
                    .expect("physical source")
                    .as_path()
            )
        );
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
        fs::write(root.path().join("cursor-agent"), [0xcf, 0xfa, 0xed, 0xfe])
            .expect("binary identity fixture");
        for file in ["index.js", "7096.index.js", "5622.index.js"] {
            fs::write(root.path().join(file), "initial native source").expect("source fixture");
        }
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
}
