//! Follow successful migration pages; a failed page is never retried.
use collaboration_central_admin_protocol::{
    CopyMigrationItem, CopyMigrationPage, CopyMigrationResult, CopyPageQuery, OwnerOperationFailure,
};
use collaboration_client::{CentralAdminAnswer, CentralAdminError};
use message_board::ProjectId;
use serde::Serialize;
use std::{future::Future, io::Write};
#[derive(Default, Serialize)]
struct MigrationCounts {
    migrated: u64,
    current: u64,
    failed: u64,
    retired: u64,
}
impl MigrationCounts {
    fn observe(&mut self, result: &CopyMigrationResult) {
        match result {
            CopyMigrationResult::Migrated { .. } => self.migrated += 1,
            CopyMigrationResult::Current { .. } => self.current += 1,
            CopyMigrationResult::Failed { .. } => self.failed += 1,
            CopyMigrationResult::Retired => self.retired += 1,
        }
    }
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum MigrationStop {
    Http(OwnerOperationFailure),
    Unauthorized,
    InvalidPage,
    UnexpectedAnswer,
    Unavailable { detail: String },
    OutcomeUnknown { detail: String },
    InvalidRequest { detail: String },
}
impl From<CentralAdminError> for MigrationStop {
    fn from(error: CentralAdminError) -> Self {
        match error {
            CentralAdminError::OutcomeUnknown(detail) => Self::OutcomeUnknown { detail },
            CentralAdminError::Unavailable(detail) => Self::Unavailable { detail },
            error @ (CentralAdminError::InvalidCentralUrl
            | CentralAdminError::InvalidOwnerToken
            | CentralAdminError::InvalidRequest(_)) => Self::InvalidRequest {
                detail: error.to_string(),
            },
        }
    }
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum MigrationCompletion {
    Completed,
    Stopped {
        resume_after: Option<ProjectId>,
        error: MigrationStop,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum MigrationEvent {
    Copy(CopyMigrationItem),
    Summary {
        counts: MigrationCounts,
        completion: MigrationCompletion,
    },
}
fn emit(output: &mut impl Write, event: &MigrationEvent, json: bool) -> Result<(), std::io::Error> {
    if json {
        serde_json::to_writer(&mut *output, event).map_err(std::io::Error::other)?;
        writeln!(output)?;
    } else {
        match event {
            MigrationEvent::Copy(item) => writeln!(
                output,
                "{}",
                serde_json::to_string(item).map_err(std::io::Error::other)?
            )?,
            MigrationEvent::Summary { counts, completion } => {
                writeln!(
                    output,
                    "Migrated: {}; current: {}; failed: {}; retired: {}",
                    counts.migrated, counts.current, counts.failed, counts.retired
                )?;
                match completion {
                    MigrationCompletion::Completed => {
                        writeln!(output, "Migration sweep completed.")?
                    }
                    MigrationCompletion::Stopped {
                        resume_after,
                        error,
                    } => {
                        writeln!(
                            output,
                            "Migration sweep stopped: {}",
                            serde_json::to_string(error).map_err(std::io::Error::other)?
                        )?;
                        match resume_after {
                            Some(after) => {
                                writeln!(output, "Resume with --after {}", after.as_str())?
                            }
                            None => writeln!(output, "Resume from the beginning (no --after).")?,
                        }
                    }
                }
            }
        }
    }
    output.flush()
}
fn valid_page(page: &CopyMigrationPage, after: Option<&ProjectId>, limit: Option<u16>) -> bool {
    if page.results.len() > usize::from(limit.unwrap_or(25)) {
        return false;
    }
    let mut previous = after;
    for item in &page.results {
        if previous.is_some_and(|previous| item.project.as_str() <= previous.as_str()) {
            return false;
        }
        previous = Some(&item.project);
    }
    match &page.next_after {
        Some(next) => page
            .results
            .last()
            .is_some_and(|item| &item.project == next),
        None => true,
    }
}
pub(super) async fn run<Fetch, PageFuture>(
    query: CopyPageQuery,
    mut fetch: Fetch,
    output: &mut impl Write,
    json: bool,
) -> i32
where
    Fetch: FnMut(CopyPageQuery) -> PageFuture,
    PageFuture: Future<Output = Result<CentralAdminAnswer, CentralAdminError>>,
{
    let mut after = query.after;
    let mut counts = MigrationCounts::default();
    let completion = loop {
        let page = match fetch(CopyPageQuery {
            after: after.clone(),
            limit: query.limit,
        })
        .await
        {
            Ok(CentralAdminAnswer::Migrate(page)) => page,
            Ok(CentralAdminAnswer::Failure(error)) => {
                break MigrationCompletion::Stopped {
                    resume_after: after,
                    error: MigrationStop::Http(error),
                };
            }
            Ok(CentralAdminAnswer::Unauthorized) => {
                break MigrationCompletion::Stopped {
                    resume_after: after,
                    error: MigrationStop::Unauthorized,
                };
            }
            Ok(_) => {
                break MigrationCompletion::Stopped {
                    resume_after: after,
                    error: MigrationStop::UnexpectedAnswer,
                };
            }
            Err(error) => {
                break MigrationCompletion::Stopped {
                    resume_after: after,
                    error: error.into(),
                };
            }
        };
        // Ascending/exclusive pages are the server contract. Refuse a broken
        // cursor instead of looping or skipping an operator's resume boundary.
        if !valid_page(&page, after.as_ref(), query.limit) {
            break MigrationCompletion::Stopped {
                resume_after: after,
                error: MigrationStop::InvalidPage,
            };
        }
        for item in page.results {
            counts.observe(&item.result);
            if emit(output, &MigrationEvent::Copy(item), json).is_err() {
                return 3;
            }
        }
        after = page.next_after;
        if after.is_none() {
            break MigrationCompletion::Completed;
        }
    };
    let code = if matches!(&completion, MigrationCompletion::Stopped { .. }) {
        3
    } else if counts.failed > 0 {
        4
    } else {
        0
    };
    if emit(
        output,
        &MigrationEvent::Summary { counts, completion },
        json,
    )
    .is_err()
    {
        return 3;
    }
    code
}
#[cfg(test)]
#[path = "central_migration_tests.rs"]
mod tests;
