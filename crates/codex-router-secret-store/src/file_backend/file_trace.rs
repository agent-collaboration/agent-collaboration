//! Fixture-only observations of real file I/O; captured contents stay redacted in Debug.
use super::file_write_observer::FileWriteObserver;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDurabilityTraceEvent {
    TemporaryFileWritten { path: PathBuf },
    TemporaryFileSynced { path: PathBuf },
    FileRenamed { from: PathBuf, to: PathBuf },
    DirectorySyncAttempted { path: PathBuf },
}

#[derive(Clone, Default)]
pub struct FileDurabilityTrace(Arc<Mutex<Vec<FileDurabilityTraceEvent>>>);
impl std::fmt::Debug for FileDurabilityTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FileDurabilityTrace([PATHS REDACTED])")
    }
}
impl FileDurabilityTrace {
    pub fn events(&self) -> Vec<FileDurabilityTraceEvent> {
        self.0
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
    pub(super) fn directory_sync_attempted(&self, path: &Path) {
        self.record(FileDurabilityTraceEvent::DirectorySyncAttempted {
            path: path.to_path_buf(),
        });
    }
    fn record(&self, event: FileDurabilityTraceEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}
impl FileWriteObserver for FileDurabilityTrace {
    fn temporary_file_written(&self, path: &Path, _: &[u8]) {
        self.record(FileDurabilityTraceEvent::TemporaryFileWritten {
            path: path.to_path_buf(),
        });
    }
    fn temporary_file_synced(&self, path: &Path) {
        self.record(FileDurabilityTraceEvent::TemporaryFileSynced {
            path: path.to_path_buf(),
        });
    }
    fn file_renamed(&self, from: &Path, to: &Path) {
        self.record(FileDurabilityTraceEvent::FileRenamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }
    fn file_removed(&self, _: &Path) {}
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileReadTraceEvent {
    FileOpenAttempted { path: PathBuf },
    DirectoryOpenAttempted { path: PathBuf },
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Default)]
pub struct FileReadTrace(Arc<Mutex<Vec<FileReadTraceEvent>>>);

#[cfg(any(test, feature = "test-support"))]
impl std::fmt::Debug for FileReadTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FileReadTrace([PATHS REDACTED])")
    }
}

#[cfg(any(test, feature = "test-support"))]
impl FileReadTrace {
    pub fn events(&self) -> Vec<FileReadTraceEvent> {
        self.0
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub(super) fn file_open_attempted(&self, path: &Path) {
        self.record(FileReadTraceEvent::FileOpenAttempted {
            path: path.to_path_buf(),
        });
    }

    pub(super) fn directory_open_attempted(&self, path: &Path) {
        self.record(FileReadTraceEvent::DirectoryOpenAttempted {
            path: path.to_path_buf(),
        });
    }

    pub(super) fn record(&self, event: FileReadTraceEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Default)]
pub struct FileWriteTrace(Arc<Mutex<Vec<FileWriteTraceEvent>>>);

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Eq, PartialEq)]
pub enum FileWriteTraceEvent {
    TemporaryFileWritten { path: PathBuf, contents: Vec<u8> },
    FileRenamed { from: PathBuf, to: PathBuf },
    FileRemoved { path: PathBuf },
}

#[cfg(any(test, feature = "test-support"))]
impl std::fmt::Debug for FileWriteTraceEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TemporaryFileWritten { path, .. } => formatter
                .debug_struct("TemporaryFileWritten")
                .field("path", path)
                .field("contents", &"[REDACTED]")
                .finish(),
            Self::FileRenamed { from, to } => formatter
                .debug_struct("FileRenamed")
                .field("from", from)
                .field("to", to)
                .finish(),
            Self::FileRemoved { path } => formatter
                .debug_struct("FileRemoved")
                .field("path", path)
                .finish(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl std::fmt::Debug for FileWriteTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FileWriteTrace([REDACTED])")
    }
}

#[cfg(any(test, feature = "test-support"))]
impl FileWriteTrace {
    pub fn events(&self) -> Vec<FileWriteTraceEvent> {
        self.0
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub(super) fn record(&self, event: FileWriteTraceEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl FileWriteObserver for FileWriteTrace {
    fn temporary_file_written(&self, path: &Path, contents: &[u8]) {
        self.record(FileWriteTraceEvent::TemporaryFileWritten {
            path: path.to_path_buf(),
            contents: contents.to_vec(),
        });
    }

    fn file_renamed(&self, from: &Path, to: &Path) {
        self.record(FileWriteTraceEvent::FileRenamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }

    #[cfg(any(test, feature = "test-support"))]
    fn file_removed(&self, path: &Path) {
        self.record(FileWriteTraceEvent::FileRemoved {
            path: path.to_path_buf(),
        });
    }
}
