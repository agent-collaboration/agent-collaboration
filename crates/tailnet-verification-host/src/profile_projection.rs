//! Native profile projection for the separately commissioned verification driver.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum ProfileGuardError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("debug profile conflict; newer bytes left untouched")]
    Conflict,
}

impl ProfileGuardError {
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "ValueError",
            Self::Conflict => "RuntimeError",
            Self::Io(error) => match error.kind() {
                std::io::ErrorKind::WouldBlock => "BlockingIOError",
                std::io::ErrorKind::PermissionDenied => "PermissionError",
                std::io::ErrorKind::NotFound => "FileNotFoundError",
                _ => "OSError",
            },
        }
    }
}

pub struct ProfileProjection {
    path: PathBuf,
    profile: File,
    identity: Metadata,
    original: Vec<u8>,
    projected: Vec<u8>,
}

fn endpoint_ranges(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    const PREFIX: &[u8] = b"http://127.0.0.1:";
    let mut ranges = Vec::new();
    for (start, candidate) in bytes.windows(PREFIX.len()).enumerate() {
        if candidate != PREFIX {
            continue;
        }
        let Some(tail) = bytes.get(start + PREFIX.len()..) else {
            continue;
        };
        let digits = tail.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits > 0 && tail.get(digits..digits + 3) == Some(b"/v1") {
            ranges.push(start..start + PREFIX.len() + digits + 3);
        }
    }
    ranges
}

impl ProfileProjection {
    pub fn acquire(path: &Path, expected_hash: &str, port: u16) -> Result<Self, ProfileGuardError> {
        let no_follow = i32::try_from(rustix::fs::OFlags::NOFOLLOW.bits())
            .map_err(|_| ProfileGuardError::Invalid("O_NOFOLLOW unavailable"))?;
        let mut profile = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(no_follow)
            .open(path)?;
        fs2::FileExt::try_lock_exclusive(&profile)?;
        let identity = profile.metadata()?;
        if identity.uid() != rustix::process::getuid().as_raw()
            || !identity.is_file()
            || identity.mode() & 0o022 != 0
        {
            return Err(ProfileGuardError::Invalid(
                "unsafe debug profile owner or mode",
            ));
        }
        let mut original = Vec::new();
        profile.read_to_end(&mut original)?;
        if format!("{:x}", Sha256::digest(&original)) != expected_hash {
            return Err(ProfileGuardError::Invalid("debug profile hash mismatch"));
        }
        let ranges = endpoint_ranges(&original);
        let [range] = ranges.as_slice() else {
            return Err(ProfileGuardError::Invalid(
                "debug profile must contain exactly one loopback endpoint",
            ));
        };
        let mut projected = original
            .get(..range.start)
            .ok_or(ProfileGuardError::Invalid("profile endpoint range"))?
            .to_vec();
        projected.extend_from_slice(format!("http://127.0.0.1:{port}/v1").as_bytes());
        projected.extend_from_slice(
            original
                .get(range.end..)
                .ok_or(ProfileGuardError::Invalid("profile endpoint range"))?,
        );
        let mut projection = Self {
            path: path.to_path_buf(),
            profile,
            identity,
            original,
            projected,
        };
        projection.replace_guarded(false)?;
        Ok(projection)
    }

    fn replace_guarded(&mut self, restoring: bool) -> Result<(), ProfileGuardError> {
        let current = self.path.symlink_metadata()?;
        self.profile.seek(SeekFrom::Start(0))?;
        let mut observed = Vec::new();
        self.profile.read_to_end(&mut observed)?;
        let (expected, replacement) = if restoring {
            (&self.projected, &self.original)
        } else {
            (&self.original, &self.projected)
        };
        if current.ino() != self.identity.ino()
            || current.dev() != self.identity.dev()
            || current.is_symlink()
            || &observed != expected
        {
            return Err(ProfileGuardError::Conflict);
        }
        self.profile.seek(SeekFrom::Start(0))?;
        self.profile.write_all(replacement)?;
        self.profile.set_len(
            u64::try_from(replacement.len())
                .map_err(|_| ProfileGuardError::Invalid("profile exceeds native file size"))?,
        )?;
        self.profile.sync_all()?;
        Ok(())
    }

    pub fn restore(mut self) -> Result<(), ProfileGuardError> {
        self.replace_guarded(true)
    }
}
