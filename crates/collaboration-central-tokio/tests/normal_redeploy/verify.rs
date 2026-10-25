//! Correlated complete original-socket read precedes the honest reconnect and replay.
use super::*;

impl NormalRedeployState {
    async fn read_surviving_socket(&mut self) -> ConformanceResult<()> {
        let frame = creation::send(
            &mut self.original_target,
            OperationRequest::Replication(ReplicationRequest::SnapshotPage {
                database: self.project,
                cursor: None,
            }),
        )
        .await?;
        let page = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match transport::next(&mut self.original_target).await? {
                    ServerFrame::Notice { .. } => {}
                    ServerFrame::Answer { frame: found, .. } if found == self.in_flight_frame => {}
                    ServerFrame::Answer {
                        frame: found,
                        body:
                            OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(
                                SnapshotRead::Page(page),
                            )),
                    } if found == frame
                        && page.database == self.project
                        && page.table == "records"
                        && page.schema.version == 1
                        && page.next.is_none() =>
                    {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(page);
                    }
                    other => {
                        return Err(format!(
                            "surviving socket lost its correlated complete read: {}",
                            serde_json::to_string(&other)?
                        )
                        .into());
                    }
                }
            }
        })
        .await??;
        let before = transport::expected(&[
            (1, "a-1"),
            (3, "imported-3"),
            (4, "a-4"),
            (5, "a-5"),
            (6, "a-6"),
            (7, "b-7"),
        ])?;
        let after = transport::expected(&[
            (1, "a-1"),
            (3, "imported-3"),
            (4, "a-4"),
            (5, "a-5"),
            (6, "a-6"),
            (7, "b-7"),
            (8, "b-8"),
        ])?;
        if !((page.position == position(4, 1)? && page.rows == before)
            || (page.position == position(4, 2)? && page.rows == after))
        {
            return Err(format!("surviving socket did not return either old exact complete in-flight prefix: {page:?}").into());
        }
        println!(
            "NORMAL_REDEPLOY_STAGE same_original_B_socket_correlated_exact_complete_prefix_before_any_new_Hello passed"
        );
        Ok(())
    }
    async fn assert_original_attachment(
        &self,
        platform: &collaboration_central_tokio::TokioCentralPlatform,
    ) -> ConformanceResult<()> {
        let peer = PeerId::try_from(TARGET_PEER.to_owned())?;
        let report = platform.conformance_schema_report(peer).await?;
        let expected = ReportSchemaSupport {
            peer,
            session_id: self.target_session,
            attach_seq: AttachSequence::new(1)?,
            support: SchemaSupport { max_version: 1 },
        };
        if report != Some(expected) {
            return Err(format!(
                "normal-state read must precede any replacement Hello: {report:?}"
            )
            .into());
        }
        Ok(())
    }
    pub(in super::super) async fn verify_surviving(
        &mut self,
        base: &str,
        platform: &collaboration_central_tokio::TokioCentralPlatform,
    ) -> ConformanceResult<()> {
        self.assert_original_attachment(platform).await?;
        self.read_surviving_socket().await?;
        self.assert_original_attachment(platform).await?;
        self.verify_after_reconnect(base).await?;
        println!(
            "NORMAL_REDEPLOY_CASE exact_same_socket_read_then_honest_reconnect_replay_rows_and_stale_cutoff passed"
        );
        Ok(())
    }
    pub(super) async fn verify_after_reconnect(&mut self, base: &str) -> ConformanceResult<()> {
        let mut replacement =
            transport::reconnect(base, TARGET_CREDENTIAL, self.target_session).await?;
        let proof = async {
            transport::acked(&mut replacement, &self.in_flight, position(4, 2)?).await?;
            transport::acked(
                &mut replacement,
                &piece(
                    self.project,
                    position(4, 2)?,
                    position(4, 3)?,
                    vec![row(9, "b-9")?],
                )?,
                position(4, 3)?,
            )
            .await?;
            transport::exact(
                &mut replacement,
                self.project,
                position(4, 3)?,
                &[
                    (1, "a-1"),
                    (3, "imported-3"),
                    (4, "a-4"),
                    (5, "a-5"),
                    (6, "a-6"),
                    (7, "b-7"),
                    (8, "b-8"),
                    (9, "b-9"),
                ],
            )
            .await?;
            let http = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?;
            let replay = owner_registry_scenarios::post(
                &http,
                &format!("{base}/admin/v1/projects/{}/takeover", self.project),
                &self.takeover,
            )
            .await?;
            if replay
                != json!({"kind":"granted","value":{"newEpoch":4,"start":{"epoch":2,"sequence":3}}})
            {
                return Err("old completed Takeover replay changed after reconnect".into());
            }
            let mut old = transport::reconnect(base, OLD_CREDENTIAL, self.old_session).await?;
            let stale = creation::operation_answer(
                &mut old,
                OperationRequest::Replication(ReplicationRequest::Piece(self.stale.clone())),
            )
            .await;
            let closed = old.close(reqwest_websocket::CloseCode::Normal, None).await;
            if !matches!(stale?,
                OperationAnswer::Replication(ReplicationAnswer::Refused(
                    PieceRefusal::StaleEpoch { newer, ended_at }
                )) if newer == Epoch::new(4)? && ended_at == position(2, 3)?
            ) {
                return Err("old holder lost stale4/cutoff2:3 after reconnect".into());
            }
            closed?;
            println!(
                "NORMAL_REDEPLOY_STAGE honest_reconnect_replay_full_rows_and_stale_cutoff passed"
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = replacement
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await;
        proof?;
        closed?;
        Ok(())
    }
}
