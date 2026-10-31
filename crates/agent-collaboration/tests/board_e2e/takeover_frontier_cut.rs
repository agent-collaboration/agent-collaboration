//! Hold or acknowledge the actual settled-frontier transaction before takeover.
use super::super::{
    central_pair::central,
    fixture::{BoardFixture, at},
    takeover_controls::{self as controls, BOUND, EndPrefix},
};
use board_central_test_support::{DispatchCallSelector, RuntimeControl};
use board_sync_protocol::BoardAnnotation;
use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::{ReplicationPosition, ReplicationRequest};
use fleet_sync_writer::CapturedTransaction;
use message_board_storage::CapturedProjectChanges;

#[derive(Clone, Copy, Debug)]
pub enum FrontierCut {
    // Exclude the first frontier that covers the original handoff.
    Unsettled,
    // Include the acknowledged frontier transaction in the takeover snapshot.
    Settled,
}

#[derive(Clone, Copy)]
pub enum RemoteEndPrefix {
    Discarded,
    Retained(FrontierCut),
    LostReceipt,
}

impl RemoteEndPrefix {
    pub(super) fn end_prefix(self) -> EndPrefix {
        match self {
            Self::Discarded => EndPrefix::Discarded,
            Self::Retained(_) => EndPrefix::Retained,
            Self::LostReceipt => EndPrefix::LostReceipt,
        }
    }

    pub(super) fn requires_local_replay(self) -> bool {
        matches!(
            self,
            Self::Retained(FrontierCut::Unsettled) | Self::LostReceipt
        )
    }
}

pub struct FrontierCapture {
    pub position: ReplicationPosition,
    pub settled_through: message_board::HandoffSeq,
}

pub(super) struct RemoteEndCut {
    pub pre_end_ack: ReplicationPosition,
    pub end: message_board::FeedCursor,
    pub consequence: EndCutConsequence,
}

pub(super) enum EndCutConsequence {
    Discarded,
    Retained {
        cut: FrontierCut,
        frontier: FrontierCapture,
    },
    LostReceipt(serde_json::Value),
}

pub async fn establish(
    fixture: &mut BoardFixture,
    cut: FrontierCut,
) -> FixtureResult<FrontierCapture> {
    let project = fixture.project.clone();
    let host = fixture.host.take().ok_or("predecessor Host absent")?;
    let result = tokio::time::timeout(BOUND, async {
        loop {
            let copying = host.replicate_project(&project);
            tokio::pin!(copying);
            loop {
                tokio::select! {
                    copied = &mut copying => {
                        copied?;
                        // A pass can finish before the publisher commits its frontier.
                        // Start another actual copy pass, without a cadence assumption.
                        break;
                    }
                    held = controls::held(fixture, controls::connection(fixture)?) => {
                        let held = held?;
                        if let Some(frontier) = frontier_capture(&held, cut)? {
                            if matches!(cut, FrontierCut::Settled) {
                                controls::resume(fixture, controls::connection(fixture)?).await?;
                                copying.await?;
                                controls::confirm_copy_ack(fixture, frontier.position).await?;
                            }
                            // In the Unsettled cut, leave the actual Copy held and
                            // abandon only its stdin observation. The caller crashes
                            // this Host before issuing another stdin request.
                            return Ok(frontier);
                        }
                        central(fixture)?.runtime_control(controls::connection(fixture)?, RuntimeControl::ResumeDispatchAndHoldNextSelected {
                            destination: controls::copy(fixture)?, call: DispatchCallSelector::CopyWrite,
                        }).await?;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    }).await;
    fixture.host = Some(host);
    result
        .map_err(|error| format!("CI6 establishing {cut:?} frontier Copy prefix: {error}"))?
        .map_err(|error: Box<dyn std::error::Error + Send + Sync>| {
            format!("CI6 establishing {cut:?} frontier Copy prefix: {error}").into()
        })
}

fn frontier_capture(
    held: &serde_json::Value,
    cut: FrontierCut,
) -> FixtureResult<Option<FrontierCapture>> {
    let request: ReplicationRequest<BoardAnnotation> =
        serde_json::from_value(at(held, "/request/request")?.clone())?;
    let ReplicationRequest::Piece(piece) = request else {
        return Err("frontier gate requires an actual Copy Piece".into());
    };
    for transaction in &piece.transactions {
        let position = ReplicationPosition {
            epoch: piece.epoch,
            sequence: transaction.commit_id(),
        };
        let changes = CapturedProjectChanges::decode(&CapturedTransaction {
            position,
            transaction,
        })?;
        if let Some(through) = changes.settled_through {
            // Original2 must remain above the seed floor when replay is owed.
            // The settled case includes end group3 as well.
            let required = match cut {
                FrontierCut::Unsettled => 2,
                FrontierCut::Settled => 3,
            };
            if through >= required {
                return Ok(Some(FrontierCapture {
                    position,
                    settled_through: through.try_into()?,
                }));
            }
        }
    }
    Ok(None)
}
