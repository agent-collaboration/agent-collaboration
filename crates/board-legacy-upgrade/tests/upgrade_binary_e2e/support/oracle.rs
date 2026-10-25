use super::{E2eResult, promote_error};
use crate::{
    post_selection::{ReaderCutoffs, SelectionMode, derive_reader_cutoffs, selected_post_ids},
    sealed_source::read_sealed_copy,
};
use std::path::Path;

pub struct LegacySelections {
    pub latest: Vec<String>,
    pub unread: Vec<String>,
    pub cutoffs: ReaderCutoffs,
}

pub fn read_legacy_selections(
    source: &Path,
    source_root: &Path,
    reader: &str,
    topic: &str,
) -> E2eResult<LegacySelections> {
    let sealed = read_sealed_copy(source, source_root).map_err(promote_error)?;
    Ok(LegacySelections {
        latest: selected_post_ids(&sealed.rows, reader, topic, SelectionMode::Latest),
        unread: selected_post_ids(&sealed.rows, reader, topic, SelectionMode::Unread),
        cutoffs: derive_reader_cutoffs(&sealed.rows, reader, topic),
    })
}
