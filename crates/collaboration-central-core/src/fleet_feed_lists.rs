//! Validate immutable ordered repository vectors against their feed parent and count.
use crate::{
    fleet_column_codec::{canonical, optional_text, text, unsigned},
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_fleet_feed_lists::{StoredFeedAdrFile, StoredFeedRepositoryRef},
};
use durable_steps::StoreError;
use message_board::{ArtifactRef, RepositoryRef};

pub(crate) fn repositories(
    rows: Vec<StoredFeedRepositoryRef>,
    parent: &str,
    count: u64,
) -> Result<Vec<RepositoryRef>, StoreError> {
    check_count(count, rows.len())?;
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            check_private_id(
                &text(row.row_id, row.row_id_class, "feed.repository.id")?,
                "feed.repository.id",
            )?;
            if text(row.entry_id, row.entry_id_class, "feed.repository.parent")? != parent
                || usize::try_from(unsigned(
                    row.ordinal,
                    row.ordinal_class,
                    "feed.repository.ordinal",
                )?)
                .ok()
                    != Some(index)
            {
                return Err(corrupt(
                    "feed.repositories",
                    "wrong parent or a noncontiguous ordinal",
                ));
            }
            check_millis(
                row.created_at,
                row.created_at_class,
                "feed.repository.created_at",
            )?;
            check_millis(
                row.updated_at,
                row.updated_at_class,
                "feed.repository.updated_at",
            )?;
            match (
                text(
                    row.reference_kind,
                    row.reference_kind_class,
                    "feed.repository.kind",
                )?
                .as_str(),
                optional_text(
                    row.normalized_origin,
                    row.normalized_origin_class,
                    "feed.repository.origin",
                )?,
                optional_text(
                    row.service_id,
                    row.service_id_class,
                    "feed.repository.service",
                )?,
                optional_text(
                    row.common_directory,
                    row.common_directory_class,
                    "feed.repository.directory",
                )?,
            ) {
                ("origin", Some(origin), None, None) => Ok(RepositoryRef::Origin {
                    normalized_origin: canonical(origin, "feed.repository.origin")?,
                }),
                ("local", None, Some(service), Some(directory)) => Ok(RepositoryRef::Local {
                    service_id: canonical(service, "feed.repository.service")?,
                    common_directory: canonical(directory, "feed.repository.directory")?,
                }),
                _ => Err(corrupt(
                    "feed.repository.reference",
                    "unknown kind or inactive fields",
                )),
            }
        })
        .collect()
}

pub(crate) fn files(
    rows: Vec<StoredFeedAdrFile>,
    parent: &str,
    count: u64,
) -> Result<Vec<ArtifactRef>, StoreError> {
    check_count(count, rows.len())?;
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            check_private_id(
                &text(row.row_id, row.row_id_class, "feed.file.id")?,
                "feed.file.id",
            )?;
            if text(row.entry_id, row.entry_id_class, "feed.file.parent")? != parent
                || usize::try_from(unsigned(
                    row.ordinal,
                    row.ordinal_class,
                    "feed.file.ordinal",
                )?)
                .ok()
                    != Some(index)
            {
                return Err(corrupt(
                    "feed.files",
                    "wrong parent or a noncontiguous ordinal",
                ));
            }
            check_millis(row.created_at, row.created_at_class, "feed.file.created_at")?;
            check_millis(row.updated_at, row.updated_at_class, "feed.file.updated_at")?;
            Ok(ArtifactRef {
                repository: canonical(
                    text(
                        row.repository_origin,
                        row.repository_origin_class,
                        "feed.file.repository",
                    )?,
                    "feed.file.repository",
                )?,
                path: text(row.path, row.path_class, "feed.file.path")?,
                commit: optional_text(row.commit_sha, row.commit_sha_class, "feed.file.commit")?
                    .map(|value| canonical(value, "feed.file.commit"))
                    .transpose()?,
            })
        })
        .collect()
}

fn check_count(expected: u64, actual: usize) -> Result<(), StoreError> {
    if expected > 16 || usize::try_from(expected).ok() != Some(actual) {
        return Err(corrupt(
            "feed.children",
            "child rows do not match the bounded parent count",
        ));
    }
    Ok(())
}
