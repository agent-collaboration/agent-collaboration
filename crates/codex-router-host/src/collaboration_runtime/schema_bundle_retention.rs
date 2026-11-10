//! The schema bundle a backend uses is published and marked used; bundles no
//! start has used for 30 days are removed (R16).

use std::{io, path::Path, time::Duration, time::SystemTime};

/// A bundle unused for this long is removed.
const SCHEMA_BUNDLE_UNUSED_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Publishes `in_use` (recording this use), then removes bundles unused for
/// 30 days. The bundle in use is never removed. A failed removal is logged and
/// retried the next time a bundle is published; publication still succeeds.
pub(super) fn publish_in_use_schema_bundle(
    directory: &Path,
    in_use: &codex_native_integration::NativeSchemaBundle,
) -> io::Result<()> {
    let now = SystemTime::now();
    in_use.publish(directory, now).map_err(io::Error::other)?;
    let retirement = codex_native_integration::retire_unused_schema_bundles(
        directory,
        in_use,
        now,
        SCHEMA_BUNDLE_UNUSED_RETENTION,
    );
    for (path, error) in &retirement.failures {
        tracing::warn!(
            bundle = %path.display(),
            %error,
            "unused schema bundle could not be removed; the next publication retries"
        );
    }
    Ok(())
}
