//! Reframe snapshot transport headers while retaining source paging and row bytes.
use fleet_protocol::{
    DigestError, Epoch, EpochError, PageDigest, SnapshotCursor, SnapshotManifest, SnapshotPage,
};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotTransportError {
    #[error("snapshot page does not belong to the pinned source manifest")]
    PageHeaderMismatch,
    #[error("snapshot cursor does not belong to the pinned source position")]
    CursorPositionMismatch,
    #[error("invalid import epoch")]
    Epoch(#[from] EpochError),
    #[error("snapshot page could not be sealed")]
    Digest(#[from] DigestError),
}

pub struct PreparedSnapshotPage {
    pub source_next: Option<SnapshotCursor>,
    pub contents: SnapshotPage,
    pub digest: PageDigest,
}

pub fn reframe_import_manifest(
    mut manifest: SnapshotManifest,
) -> Result<SnapshotManifest, SnapshotTransportError> {
    manifest.position.epoch = Epoch::new(1)?;
    Ok(manifest)
}

pub fn reframe_import_page(
    mut page: SnapshotPage,
    source: &SnapshotManifest,
) -> Result<PreparedSnapshotPage, SnapshotTransportError> {
    if page.database != source.database
        || page.position != source.position
        || page.schema != source.schema
    {
        return Err(SnapshotTransportError::PageHeaderMismatch);
    }
    if page
        .next
        .as_ref()
        .is_some_and(|cursor| cursor.position != source.position)
    {
        return Err(SnapshotTransportError::CursorPositionMismatch);
    }
    let source_next = page.next.clone();
    page.position.epoch = Epoch::new(1)?;
    if let Some(next) = &mut page.next {
        next.position = page.position;
    }
    Ok(PreparedSnapshotPage {
        source_next,
        digest: PageDigest::of(&page)?,
        contents: page,
    })
}

#[cfg(test)]
#[path = "snapshot_transport_test.rs"]
mod tests;
