//! Owns disposable native stores until their readback and snapshot work is closed.
use crate::{
    UpgradeError,
    import_error::invalid,
    legacy_reader::read_legacy,
    legacy_rows::{LegacyProject, LegacyStore},
    project_transform::transform,
    *,
};
use board_fleet_sync::BoardProjectSchema;
use fleet_admission::{FleetImportBundle, FleetImportRecord, ImportProvenance};
use fleet_sync_lease::{LeaseHolder, LeasePolicy};
use fleet_sync_protocol::{DatabaseId, PeerId};
use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};
use message_board::*;
use message_board_storage::{ProjectStore, apply_project_import};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub struct PreparedProject {
    pub store: ProjectStore<BoardProjectSchema>,
    pub database: Arc<ReplicatedDatabase<BoardProjectSchema>>,
}
pub struct PreparedUpgrade {
    pub report: UpgradeReport,
    pub projects: Vec<PreparedProject>,
    pub message_activity: MessageActivityMap,
    directory: tempfile::TempDir,
}
impl PreparedUpgrade {
    pub async fn close(self) {
        for project in &self.projects {
            project.database.close().await;
        }
        drop(self.projects);
        drop(self.directory);
    }
}

pub async fn prepare_sources(
    sources: &[PathBuf],
    provenance: ImportProvenance,
    owner: HumanId,
) -> Result<PreparedUpgrade, UpgradeError> {
    let mut stores = Vec::new();
    for source in sources {
        let started = Instant::now();
        let store = read_legacy(source.clone()).await?;
        tracing::info!(target: "board_legacy_upgrade::progress", phase = "read", posts = store.posts.len(), activity_rows = store.activity.len(), projects = store.projects.len(), elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
        stores.push(store);
    }
    let collisions = find_collisions(&stores);
    let mut fleet = FleetImportBundle {
        declared_owner: owner.clone(),
        provenance: provenance.clone(),
        records: Vec::new(),
    };
    let mut refused = Vec::new();
    let mut fleet_records_by_id = BTreeMap::new();
    for source in &stores {
        for record in fleet_records(source)? {
            let reference = serde_json::to_string(&record.reference())?;
            if let Some(previous) = fleet_records_by_id.get(&reference) {
                if previous != &record {
                    refused.push(format!("importConflict: fleet {reference}"));
                }
            } else {
                fleet_records_by_id.insert(reference, record);
            }
        }
    }
    fleet.records = fleet_records_by_id.into_values().collect();
    let fleet_digest = fleet.digest()?;
    let directory = tempfile::tempdir()?;
    let mut reports = Vec::new();
    let mut projects: Vec<PreparedProject> = Vec::new();
    let mut skipped = Vec::new();
    let mut seen = BTreeMap::new();
    let mut message_activity = MessageActivityMap::default();
    for (source_index, source) in stores.iter().enumerate() {
        for project in &source.projects {
            let path = directory.path().join(format!(
                "{source_index}-{}.sqlite",
                project.topic_id.as_str()
            ));
            let result = prepare_project(source, project, &provenance, &owner, &path).await;
            let (prepared, report) = match result {
                Ok(value) => value,
                Err(error) => {
                    for project in &projects {
                        project.database.close().await;
                    }
                    return Err(error);
                }
            };
            if let Some(previous) = seen.get(&project.topic_id) {
                if previous == &report.digest {
                    skipped.push(project.topic_id.clone());
                } else {
                    refused.push(format!("importConflict: {}", project.topic_id.as_str()));
                }
                prepared.database.close().await;
                continue;
            }
            seen.insert(project.topic_id.clone(), report.digest.clone());
            if let Err(error) = message_activity.insert_project(source, project, &report.old_to_new)
            {
                prepared.database.close().await;
                for project in &projects {
                    project.database.close().await;
                }
                return Err(error);
            }
            reports.push(report);
            projects.push(prepared);
        }
    }
    let (workspace_counts, board_counts) = crate::upgrade_report::resource_counts(&fleet, &reports);
    Ok(PreparedUpgrade {
        report: UpgradeReport {
            owner,
            fleet,
            fleet_digest,
            collisions,
            projects: reports,
            workspace_counts,
            board_counts,
            skipped,
            refused,
        },
        projects,
        message_activity,
        directory,
    })
}

async fn prepare_project(
    source: &LegacyStore,
    project: &LegacyProject,
    provenance: &ImportProvenance,
    owner: &HumanId,
    path: &Path,
) -> Result<(PreparedProject, ProjectImportReport), UpgradeError> {
    let started = Instant::now();
    let transformed = transform(source, project, provenance, owner)?;
    let rows = transformed.batch.records.len();
    tracing::info!(target: "board_legacy_upgrade::progress", phase = "transformed", rows, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
    let started = Instant::now();
    let digest = crate::import_digest::semantic_import_digest(project, &transformed.batch)?;
    tracing::info!(target: "board_legacy_upgrade::progress", phase = "digest", rows, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
    let database_id = DatabaseId::new(
        uuid::Uuid::parse_str(project.topic_id.as_str())
            .map_err(|error| invalid("project_id", error))?,
    )
    .map_err(|error| invalid("project_id", error))?;
    let peer = PeerId::new(
        uuid::Uuid::parse_str(project.topic_id.as_str())
            .map_err(|error| invalid("project_id", error))?,
    )
    .map_err(|error| invalid("project_id", error))?;
    let started = Instant::now();
    let database = Arc::new(
        ReplicatedDatabase::open_with(
            path,
            database_id,
            BoardProjectSchema::new()?,
            LeaseHolder::provisional(
                database_id,
                peer,
                LeasePolicy {
                    lease_duration: Duration::from_secs(14400),
                    renew_interval: Duration::from_secs(900),
                },
            ),
            Arc::new(runtime_primitives::SystemClock),
            WriterOptions {
                allow_provisional_writes: true,
            },
        )
        .await?,
    );
    tracing::info!(target: "board_legacy_upgrade::progress", phase = "opened", rows = 0, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
    let outcome = async {
        let started = Instant::now();
        apply_project_import(
            project.topic_id.clone(),
            database.begin_write().await?,
            provenance.clone(),
            Arc::new(DerivedImportKeys::new(&project.topic_id)),
            transformed.batch,
        )
        .await?;
        tracing::info!(target: "board_legacy_upgrade::progress", phase = "staged", rows, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let store =
            ProjectStore::from_database(project.topic_id.clone(), Arc::clone(&database)).await?;
        if !matches!(
            store.schema_state(),
            message_board_storage::StoreSchemaState::Ready { .. }
        ) {
            return Err(message_board_storage::ProjectStorageError::InvalidSchema.into());
        }
        let mut snapshot = database.snapshot().await?;
        let manifest = snapshot.manifest().await?;
        snapshot.close().await?;
        let inspection = store.snapshot().await?;
        let feed_rows = inspection.high_water().position.get();
        inspection.finish().await?;
        tracing::info!(target: "board_legacy_upgrade::progress", phase = "verified", rows = feed_rows, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0);
        let workspace = source
            .boards
            .iter()
            .find(|board| board.board_id == project.board_id)
            .ok_or_else(|| invalid("board", "missing"))?
            .project_id
            .clone();
        let report = ProjectImportReport {
            project: project.topic_id.clone(),
            workspace,
            board: project.board_id.clone(),
            title: project.name.clone(),
            description: project.description.clone(),
            posts: transformed.post_count,
            feed_rows,
            imported_resolutions: transformed.resolutions,
            old_to_new: transformed.mapping,
            digest,
            manifest,
        };
        Ok((
            PreparedProject {
                store,
                database: Arc::clone(&database),
            },
            report,
        ))
    }
    .await;
    if outcome.is_err() {
        database.close().await;
    }
    outcome
}

fn fleet_records(source: &LegacyStore) -> Result<Vec<FleetImportRecord>, UpgradeError> {
    let mut records = Vec::new();
    for workspace in &source.workspaces {
        let mut repositories = Vec::new();
        for row in source
            .repositories
            .iter()
            .filter(|row| row.project_id == workspace.project_id)
        {
            let repository = match (
                row.kind.as_str(),
                &row.origin,
                &row.service_id,
                &row.common_directory,
            ) {
                ("origin", Some(origin), None, None) => RepositoryRef::Origin {
                    normalized_origin: origin.clone(),
                },
                ("local", None, Some(service_id), Some(common_directory)) => RepositoryRef::Local {
                    service_id: service_id.clone(),
                    common_directory: common_directory.clone(),
                },
                _ => return Err(invalid("repository", "fields do not match kind")),
            };
            repositories.push(repository);
        }
        records.push(FleetImportRecord::Workspace {
            id: workspace.project_id.clone(),
            name: workspace.name.clone(),
            description: workspace.description.clone(),
            repositories,
            time: RecordTime::LegacyUnknown,
        });
    }
    for board in &source.boards {
        records.push(FleetImportRecord::Board {
            id: board.board_id.clone(),
            workspace: board.project_id.clone(),
            name: board.name.clone(),
            description: board.description.clone(),
            state: board.state,
            time: RecordTime::LegacyUnknown,
        });
    }
    for project in &source.projects {
        records.push(FleetImportRecord::Directory {
            project: project.topic_id.clone(),
            board: project.board_id.clone(),
            title: project.name.clone(),
            time: RecordTime::LegacyUnknown,
        });
    }
    Ok(records)
}

fn find_collisions(stores: &[LegacyStore]) -> Vec<NameCollision> {
    let mut names: BTreeMap<
        (CollisionLocation, ResourceName),
        std::collections::BTreeSet<RecordId>,
    > = BTreeMap::new();
    for source in stores {
        for row in &source.workspaces {
            names
                .entry((CollisionLocation::Workspace, row.name.clone()))
                .or_default()
                .insert(row.project_id.clone().into());
        }
        for row in &source.boards {
            names
                .entry((
                    CollisionLocation::Board {
                        scope: row.project_id.clone(),
                    },
                    row.name.clone(),
                ))
                .or_default()
                .insert(row.board_id.clone().into());
        }
        for row in &source.projects {
            names
                .entry((
                    CollisionLocation::Project {
                        scope: row.board_id.clone(),
                    },
                    row.name.clone(),
                ))
                .or_default()
                .insert(row.topic_id.clone().into());
        }
    }
    names
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|((location, name), ids)| NameCollision {
            location,
            name,
            ids: ids.into_iter().collect(),
        })
        .collect()
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[tokio::test]
    async fn empty_legacy_project_roundtrips_unknown_metadata_without_a_creation_fact()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = read_legacy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/role-history.sqlite"),
        )
        .await?;
        let project = source
            .projects
            .iter()
            .find(|project| project.topic_id.as_str().ends_with("0016"))
            .ok_or("empty project missing")?;
        let provenance = ImportProvenance {
            installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
            machine: MachineLabel::try_from("offline import".to_owned())?,
        };
        let owner = HumanId::try_from("explicit-owner".to_owned())?;
        let directory = tempfile::tempdir()?;
        let (prepared, report) = prepare_project(
            &source,
            project,
            &provenance,
            &owner,
            &directory.path().join("project.sqlite"),
        )
        .await?;
        assert_eq!(report.feed_rows, 1);
        assert_eq!(report.posts, 0);
        let mut snapshot = prepared.store.snapshot().await?;
        let Some(Record::Project(state)) =
            snapshot.record(&project.topic_id.clone().into()).await?
        else {
            return Err("metadata missing".into());
        };
        assert_eq!(state.envelope.time, RecordTime::LegacyUnknown);
        assert_eq!(state.description, project.description);
        assert_eq!(
            state.envelope.actor,
            RecordedActor::Human { human_id: owner }
        );
        assert_eq!(
            state
                .envelope
                .position
                .project_cursor()
                .ok_or("placement missing")?
                .position
                .get(),
            1
        );
        assert!(
            snapshot
                .record_keys(RecordKind::Fact, "", 100)
                .await?
                .is_empty()
        );
        snapshot.finish().await?;
        prepared.database.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn collision_detection_keeps_both_ids_and_never_renames()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let sources = vec![
            read_legacy(root.join("legacy-complete.sqlite")).await?,
            read_legacy(root.join("collision.sqlite")).await?,
        ];
        let collisions = find_collisions(&sources);
        assert!(collisions.iter().any(|collision| matches!(
            collision.location,
            CollisionLocation::Workspace
        ) && collision.ids.len() == 2));
        assert!(collisions.iter().any(|collision| matches!(
            collision.location,
            CollisionLocation::Board { .. }
        ) && collision.ids.len() == 2));
        Ok(())
    }
}
