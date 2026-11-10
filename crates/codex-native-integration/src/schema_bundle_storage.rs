//! Immutable schema bytes are installed before their content identity is advertised.
use crate::{NativeSchemaBundle, NativeSchemaError};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

impl NativeSchemaBundle {
    /// Installs this bundle as `<digest>.json` in `directory` (or verifies the copy already
    /// there) and records `used_at` as its last use in the file's modification time.
    pub fn publish(
        &self,
        directory: &Path,
        used_at: SystemTime,
    ) -> Result<PathBuf, NativeSchemaError> {
        let metadata =
            std::fs::symlink_metadata(directory).map_err(NativeSchemaError::Filesystem)?;
        if !directory.is_absolute()
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(NativeSchemaError::InvalidDocuments);
        }
        let destination = directory.join(self.bundle_file_name()?);
        match verify_existing(&destination, self.canonical_bytes()) {
            Ok(()) => {
                record_use(&destination, used_at)?;
                return Ok(destination);
            }
            Err(NativeSchemaError::Filesystem(error))
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let temporary = directory.join(format!(
            ".schema-bundle-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(NativeSchemaError::Filesystem)?;
        let result = (|| {
            file.write_all(self.canonical_bytes())
                .map_err(NativeSchemaError::Filesystem)?;
            file.sync_all().map_err(NativeSchemaError::Filesystem)?;
            match std::fs::hard_link(&temporary, &destination) {
                Ok(()) => File::open(directory)
                    .and_then(|directory| directory.sync_all())
                    .map_err(NativeSchemaError::Filesystem)?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    verify_existing(&destination, self.canonical_bytes())?;
                }
                Err(error) => return Err(NativeSchemaError::Filesystem(error)),
            }
            Ok(destination)
        })();
        let cleanup = std::fs::remove_file(temporary).map_err(NativeSchemaError::Filesystem);
        let published = result?;
        cleanup?;
        record_use(&published, used_at)?;
        Ok(published)
    }

    /// `<lowercase hex digest>.json`, the name this bundle is published under.
    fn bundle_file_name(&self) -> Result<String, NativeSchemaError> {
        let mut filename = String::with_capacity(BUNDLE_FILE_NAME_LENGTH);
        for byte in self.digest() {
            use std::fmt::Write as _;
            write!(filename, "{byte:02x}").map_err(|_| NativeSchemaError::InvalidDocuments)?;
        }
        filename.push_str(".json");
        Ok(filename)
    }
}

const BUNDLE_FILE_NAME_LENGTH: usize = 64 + ".json".len();

/// A bundle's modification time is its last use: set when it is published and
/// whenever a later start publishes the same digest again.
fn record_use(path: &Path, used_at: SystemTime) -> Result<(), NativeSchemaError> {
    File::open(path)
        .and_then(|file| file.set_modified(used_at))
        .map_err(NativeSchemaError::Filesystem)
}

/// What one retirement pass removed and which bundles it could not remove.
#[derive(Debug, Default)]
pub struct SchemaBundleRetirement {
    pub removed: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, std::io::Error)>,
}

/// Removes schema bundles in `directory` whose last use is more than `unused_for`
/// before `now`. The bundle `in_use` is always kept. Files that are not schema
/// bundles, and bundles whose last use cannot be read, are left alone; a removal
/// that fails is reported and the bundle stays for a later pass.
pub fn retire_unused_schema_bundles(
    directory: &Path,
    in_use: &NativeSchemaBundle,
    now: SystemTime,
    unused_for: Duration,
) -> SchemaBundleRetirement {
    let mut retirement = SchemaBundleRetirement::default();
    let Ok(in_use_name) = in_use.bundle_file_name() else {
        return retirement;
    };
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            retirement.failures.push((directory.to_owned(), error));
            return retirement;
        }
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name == in_use_name || !is_bundle_file_name(name) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let Ok(last_use) = metadata.modified() else {
            continue;
        };
        let unused_long_enough = now
            .duration_since(last_use)
            .is_ok_and(|unused| unused > unused_for);
        if !metadata.is_file() || !unused_long_enough {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => retirement.removed.push(path),
            Err(error) => retirement.failures.push((path, error)),
        }
    }
    retirement.removed.sort();
    retirement
}

fn is_bundle_file_name(name: &str) -> bool {
    name.len() == BUNDLE_FILE_NAME_LENGTH
        && name.strip_suffix(".json").is_some_and(|digest| {
            digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn verify_existing(path: &Path, expected: &[u8]) -> Result<(), NativeSchemaError> {
    let metadata = std::fs::symlink_metadata(path).map_err(NativeSchemaError::Filesystem)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len()
            != u64::try_from(expected.len()).map_err(|_| NativeSchemaError::Capacity)?
    {
        return Err(NativeSchemaError::InvalidDocuments);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(NativeSchemaError::Filesystem)?
        .take(metadata.len() + 1)
        .read_to_end(&mut bytes)
        .map_err(NativeSchemaError::Filesystem)?;
    if bytes != expected {
        return Err(NativeSchemaError::InvalidDocuments);
    }
    Ok(())
}
