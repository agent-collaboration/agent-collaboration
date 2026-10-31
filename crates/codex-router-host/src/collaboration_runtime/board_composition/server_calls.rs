//! Server calls remain answerable while the machine session is attaching.
use super::project_writers::ProjectWriters;
use board_sync_protocol::{
    BoardClientAnswer, BoardConsumerProtocol, BoardPayload, BoardServerRequest, ForwardedPostKind,
    VerifiedForwardedPost,
};
use collaboration_service::{MailDeliveryRuntime, NotificationPublisher, ProjectAdmission};
use fleet_sync_protocol::{
    ClientAnswer, FrameId, ReleaseAnswer, ReleaseRefusal, ServerCall, ServerCallAnswer,
    ServerRequest, WithdrawAnswer,
};
use fleet_sync_tokio_runner::SessionRequests;
use message_board::{BoardOperation, ProjectId, ProjectRequest};
use std::{
    io,
    sync::{Arc, Weak},
};

pub(super) async fn answer_server_call(
    projects: Arc<ProjectWriters>,
    admission: Weak<ProjectAdmission>,
    mail: Weak<MailDeliveryRuntime>,
    publisher: Weak<NotificationPublisher>,
    requests: SessionRequests<BoardConsumerProtocol>,
    frame: FrameId,
    body: ServerRequest<BoardConsumerProtocol>,
) -> io::Result<()> {
    match body {
        ServerRequest::Crate(ServerCall::Release { call, request }) => {
            let project =
                ProjectId::try_from(request.database.to_string()).map_err(io::Error::other)?;
            if let Some(answer) =
                superseded_release_answer(projects.superseded_current(&project).await?.flatten())?
            {
                return requests
                    .answer(
                        frame,
                        ClientAnswer::Crate(ServerCallAnswer::Release(answer)),
                    )
                    .await
                    .map_err(io::Error::other);
            }
            projects
                .ensure(&project)
                .await?
                .runtime
                .answer_release(frame, call, &request)
                .await
                .map_err(io::Error::other)
        }
        ServerRequest::Crate(ServerCall::WithdrawRelease { call, request }) => {
            let project =
                ProjectId::try_from(request.database.to_string()).map_err(io::Error::other)?;
            if projects.writer_is_terminal(&project).await?
                || projects.superseded_current(&project).await?.is_some()
            {
                return requests
                    .answer(
                        frame,
                        ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(
                            WithdrawAnswer::NotHolder,
                        )),
                    )
                    .await
                    .map_err(io::Error::other);
            }
            projects
                .ensure(&project)
                .await?
                .runtime
                .answer_withdrawal(frame, call, &request)
                .await
                .map_err(io::Error::other)
        }
        ServerRequest::Consumer(BoardServerRequest::ForwardedPost(post)) => {
            let admission = admission
                .upgrade()
                .ok_or_else(|| io::Error::other("board admission has closed"))?;
            let (request, origin) = decode_forwarded_post(post)?;
            #[cfg(test)]
            tracing::trace!(
                target: "codex_router_host::runtime_test_forwarded_post",
                request_id = request.request_id.as_str(),
                "forwarded-post-native-admission-entered"
            );
            let reply = admission
                .submit_forwarded_post(&request, origin)
                .await
                .map_err(io::Error::other)?;
            let publisher = publisher.upgrade();
            let mail = mail.upgrade();
            collaboration_service::after_project_commit_publish_and_reconcile(
                publisher.as_deref(),
                mail.as_deref()
                    .map(MailDeliveryRuntime::subscription_delivery),
                request.actor,
                request.project_id,
            )
            .await;
            let payload = BoardPayload::encode(&reply).map_err(io::Error::other)?;
            requests
                .answer(
                    frame,
                    ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(payload)),
                )
                .await
                .map_err(io::Error::other)
        }
        ServerRequest::Crate(ServerCall::PublishStatus { publication, .. }) => {
            let mail = mail.upgrade().ok_or_else(|| {
                io::Error::other("board delivery publication owner is not attached")
            })?;
            mail.publish_status(publication)
                .await
                .map_err(io::Error::other)?;
            requests
                .answer(
                    frame,
                    ClientAnswer::Crate(ServerCallAnswer::Published(
                        fleet_sync_protocol::StatusPublicationResult::Applied,
                    )),
                )
                .await
                .map_err(io::Error::other)
        }
    }
}

fn superseded_release_answer(
    current: Option<message_board::EpochNumber>,
) -> io::Result<Option<ReleaseAnswer>> {
    current
        .map(|epoch| {
            let current = fleet_sync_protocol::Epoch::new(epoch.get()).map_err(io::Error::other)?;
            Ok(ReleaseAnswer::ReleaseRefused(ReleaseRefusal::Superseded {
                current,
            }))
        })
        .transpose()
}

fn decode_forwarded_post(
    post: VerifiedForwardedPost,
) -> io::Result<(ProjectRequest, message_board::ActorOrigin)> {
    let request: ProjectRequest = post.post.body.decode().map_err(io::Error::other)?;
    let same_family = matches!(
        (&request.operation, post.post.kind),
        (
            BoardOperation::ThreadCreate { .. },
            ForwardedPostKind::ThreadCreate
        ) | (
            BoardOperation::ThreadPost { .. },
            ForwardedPostKind::ThreadPost
        )
    );
    if request.project_id != post.project || !same_family {
        return Err(io::Error::other(
            "forwarded post project or operation does not match its envelope",
        ));
    }
    Ok((request, post.origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_sync_protocol::ForwardedPost;
    use message_board::{
        ActorOrigin, HumanId, Identity, MachineLabel, PostId, RequestId, ServiceId,
    };

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions compare authority answers; Result propagates fixture errors."
    )]
    fn superseded_release_requires_the_actual_successor_and_never_guesses_one()
    -> Result<(), Box<dyn std::error::Error>> {
        let current = fleet_sync_protocol::Epoch::new(7)?;
        assert_eq!(superseded_release_answer(None)?, None);
        assert_eq!(
            superseded_release_answer(Some(message_board::EpochNumber::try_from(7)?))?,
            Some(ReleaseAnswer::ReleaseRefused(ReleaseRefusal::Superseded {
                current
            })),
        );
        Ok(())
    }

    fn forwarded_fixture() -> Result<VerifiedForwardedPost, Box<dyn std::error::Error>> {
        let project = ProjectId::generate();
        let request: ProjectRequest = serde_json::from_value(serde_json::json!({
            "requestId":RequestId::generate(), "projectId":project,
            "protocolVersion":1,  "actingFor":null,
            "actor":{"kind":"human","humanId":"declared sender"},
            "operation":{"kind":"threadCreate","postId":PostId::generate(),
                "about":{"kind":"project","subject":{"line":"forwarded discussion"}},
                "text":"forwarded body","references":[],"participation":null,"watch":false,"addressees":[]},
        }))?;
        Ok(VerifiedForwardedPost {
            project,
            origin: ActorOrigin {
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000002".to_owned(),
                )?,
                machine: MachineLabel::try_from("fixture enrolled sender".to_owned())?,
            },
            post: ForwardedPost {
                kind: ForwardedPostKind::ThreadCreate,
                body: BoardPayload::encode(&request)?,
            },
        })
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
    )]
    fn forwarded_decoder_preserves_the_declared_actor_and_supplied_origin()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = forwarded_fixture()?;
        let expected_project = fixture.project.clone();
        let expected_origin = fixture.origin.clone();
        let (request, origin) = decode_forwarded_post(fixture)?;
        assert_eq!(request.project_id, expected_project);
        assert_eq!(
            request.actor,
            Identity::Human {
                human_id: HumanId::try_from("declared sender".to_owned())?
            }
        );
        assert_eq!(origin, expected_origin);
        Ok(())
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
    )]
    fn forwarded_decoder_refuses_mismatched_project_family_and_malformed_body()
    -> Result<(), Box<dyn std::error::Error>> {
        // The outer route and family are independent of the opaque body. Dropping
        // either validation must fail before the admission owner is called.
        let mut wrong_project = forwarded_fixture()?;
        wrong_project.project = ProjectId::generate();
        assert!(
            decode_forwarded_post(wrong_project).is_err(),
            "outer project mismatch was accepted"
        );
        let mut wrong_family = forwarded_fixture()?;
        wrong_family.post.kind = ForwardedPostKind::ThreadPost;
        assert!(
            decode_forwarded_post(wrong_family).is_err(),
            "outer post family mismatch was accepted"
        );
        let mut malformed = forwarded_fixture()?;
        malformed.post.body = BoardPayload::encode(&serde_json::json!({"unexpected":"body"}))?;
        assert!(
            decode_forwarded_post(malformed).is_err(),
            "malformed body was accepted"
        );
        Ok(())
    }
}
