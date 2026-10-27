//! Compose independent native observations without taking ownership of their lifecycle.
use crate::collaboration_application::ProviderInventoryFailure;
use claude_code_peer_messaging::{NativeClaudeJobReadError, PeerSessionSummary};
use collaboration_protocol::{
    ClaudeCodeBackgroundOrigin, ClaudeCodeInteractiveOrigin, ClaudeCodeInteractiveStatus,
    EndpointRef, NativeClaudeInventoryCoverage, NativeClaudeInventoryPart,
    NativeClaudePeerObservation, NativeInventoryUnavailableReason, NativeSessionView,
    ProviderSessionSummary, ProviderWorkingDirectory, SessionId, SessionRef,
};
use std::collections::BTreeMap;

pub(crate) struct NativeCatalogueRead {
    pub sessions: Vec<ProviderSessionSummary>,
    pub coverage: NativeClaudeInventoryCoverage,
    pub skipped_records: Option<u32>,
}

fn invalid() -> ProviderInventoryFailure {
    ProviderInventoryFailure::InvalidRequest("invalid or conflicting native background inventory")
}

pub(crate) async fn read_native_catalogue(
    claude_code_sessions: Option<
        &std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>,
    >,
    claude_background_jobs: Option<
        &std::sync::Arc<claude_code_peer_messaging::NativeClaudeJobReader>,
    >,
    endpoint: &EndpointRef,
    view: NativeSessionView,
) -> Result<NativeCatalogueRead, ProviderInventoryFailure> {
    let peers = async {
        if matches!(view, NativeSessionView::Stored) {
            return None;
        }
        let Some(registry) = claude_code_sessions.cloned() else {
            return Some(Err(()));
        };
        Some(
            match tokio::task::spawn_blocking(move || registry.live_sessions()).await {
                Ok(Ok(peers)) => Ok(peers),
                _ => Err(()),
            },
        )
    };
    let jobs = async {
        match claude_background_jobs {
            Some(reader) => reader.read_background_jobs().await,
            None => Err(NativeClaudeJobReadError::Unavailable(
                NativeInventoryUnavailableReason::ReadUnavailable,
            )),
        }
    };
    let (peers, jobs) = tokio::join!(peers, jobs);
    let mut peer_rows =
        BTreeMap::<SessionId, (ProviderWorkingDirectory, PeerSessionSummary)>::new();
    let (peer_coverage, mut skipped_records) = match peers {
        None => (NativeClaudeInventoryPart::NotRequested, None),
        Some(Err(())) => (
            NativeClaudeInventoryPart::Unavailable {
                reason: NativeInventoryUnavailableReason::ReadUnavailable,
            },
            None,
        ),
        Some(Ok(peers)) => {
            let mut skipped = peers.skipped_records;
            for peer in peers.sessions {
                let directory = match ProviderWorkingDirectory::try_from(
                    peer.cwd.to_string_lossy().into_owned(),
                ) {
                    Ok(directory) => directory,
                    Err(_) => {
                        skipped = skipped.saturating_add(1);
                        continue;
                    }
                };
                if peer_rows
                    .insert(peer.session_id.clone(), (directory, peer))
                    .is_some()
                {
                    return Err(invalid());
                }
            }
            (NativeClaudeInventoryPart::Complete, Some(skipped))
        }
    };
    let mut sessions = Vec::new();
    let background_coverage = match jobs {
        Err(NativeClaudeJobReadError::InvalidOutput) => return Err(invalid()),
        Err(NativeClaudeJobReadError::Unavailable(reason)) => {
            NativeClaudeInventoryPart::Unavailable { reason }
        }
        Ok(inventory) => {
            for job in inventory.jobs {
                let peer = peer_rows
                    .remove(&job.session_id)
                    .map(|(directory, peer)| {
                        if directory != job.working_directory {
                            return Err(invalid());
                        }
                        Ok(NativeClaudePeerObservation {
                            name: peer.name,
                            status: peer_status(&peer.status),
                            started_at: peer.started_at,
                            updated_at: peer.updated_at,
                            status_updated_at: peer.status_updated_at,
                            kind: peer.kind,
                            entrypoint: peer.entrypoint,
                        })
                    })
                    .transpose()?;
                let visible = match view {
                    NativeSessionView::Stored => true,
                    NativeSessionView::Loaded => peer.is_some() || job.state.is_loaded(),
                    NativeSessionView::Active => {
                        job.state.is_active()
                            || peer.as_ref().is_some_and(|peer| {
                                peer.status == ClaudeCodeInteractiveStatus::Busy
                            })
                    }
                };
                if visible {
                    sessions.push(ProviderSessionSummary::ClaudeCodeBackground {
                        origin: ClaudeCodeBackgroundOrigin::ClaudeCodeBackground,
                        target: SessionRef {
                            endpoint: endpoint.clone(),
                            session_id: job.session_id,
                        },
                        job_handle: job.job_handle,
                        working_directory: job.working_directory,
                        name: job.name,
                        started_at: job.started_at,
                        state: job.state,
                        status: job.status,
                        peer,
                    });
                }
            }
            if inventory.unidentified_jobs == 0 {
                NativeClaudeInventoryPart::Complete
            } else {
                NativeClaudeInventoryPart::Incomplete {
                    unidentified_jobs: inventory.unidentified_jobs,
                }
            }
        }
    };
    for (_, (working_directory, peer)) in peer_rows {
        let status = peer_status(&peer.status);
        if matches!(view, NativeSessionView::Active) && status != ClaudeCodeInteractiveStatus::Busy
        {
            continue;
        }
        sessions.push(ProviderSessionSummary::ClaudeCodeInteractive {
            origin: ClaudeCodeInteractiveOrigin::ClaudeCodeInteractive,
            target: SessionRef {
                endpoint: endpoint.clone(),
                session_id: peer.session_id,
            },
            working_directory,
            name: peer.name,
            status,
            started_at: peer.started_at,
            updated_at: peer.updated_at,
            status_updated_at: peer.status_updated_at,
            kind: peer.kind,
            entrypoint: peer.entrypoint,
        });
    }
    // Preserve the registry's existing independently published skipped count.
    if matches!(view, NativeSessionView::Stored) {
        skipped_records = None;
    }
    Ok(NativeCatalogueRead {
        sessions,
        coverage: NativeClaudeInventoryCoverage {
            peer: peer_coverage,
            background: background_coverage,
        },
        skipped_records,
    })
}

fn peer_status(
    status: &claude_code_peer_messaging::PeerSessionStatus,
) -> ClaudeCodeInteractiveStatus {
    use claude_code_peer_messaging::PeerSessionStatus;
    match status {
        PeerSessionStatus::Busy => ClaudeCodeInteractiveStatus::Busy,
        PeerSessionStatus::Idle => ClaudeCodeInteractiveStatus::Idle,
        PeerSessionStatus::Waiting => ClaudeCodeInteractiveStatus::Waiting,
        PeerSessionStatus::Shell => ClaudeCodeInteractiveStatus::Shell,
        PeerSessionStatus::Unreported => ClaudeCodeInteractiveStatus::Unreported,
        PeerSessionStatus::Other(_) => ClaudeCodeInteractiveStatus::Other,
    }
}
