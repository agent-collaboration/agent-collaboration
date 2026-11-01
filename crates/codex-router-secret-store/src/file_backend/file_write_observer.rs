//! Read-only observation of completed atomic-write stages, separate from stored content.
use std::path::Path;

pub(super) trait FileWriteObserver {
    fn temporary_file_written(&self, path: &Path, contents: &[u8]);
    fn file_renamed(&self, from: &Path, to: &Path);
    fn temporary_file_synced(&self, _path: &Path) {}
    #[cfg(any(test, feature = "test-support"))]
    fn file_removed(&self, path: &Path);
}
