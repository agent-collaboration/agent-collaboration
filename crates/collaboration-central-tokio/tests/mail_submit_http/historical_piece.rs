//! Historical cutoff authority survives two real grants and a late stale piece.
use super::registry_setup as history;
use super::{
    grants::{connect, start},
    relay::operation,
    *,
};
use board_sync_protocol::{BoardAnnotation, BoardConsumerProtocol};
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_protocol::{
    CommitSequence, DatabaseId, Epoch, OperationAnswer, PeerId, Piece, PieceContents, PieceRefusal,
    ReplicatedTransaction, ReplicationAnswer, ReplicationPosition,
};
use reqwest_websocket::CloseCode;
use serde_json::json;

const FIRST: &str = "550e8400-e29b-41d4-a716-4466554400fc";
const SECOND: &str = "550e8400-e29b-41d4-a716-4466554400fd";
const CURRENT: &str = "550e8400-e29b-41d4-a716-4466554400fe";

#[tokio::test]
async fn normal_stale_piece_keeps_its_historical_epoch_end_after_two_moves() -> ConformanceResult<()>
{
    // The setup uses real registration, two HolderChangeTasks and the real Copy.
    // Its idle release/Fleet-admission replies remain named setup stand-ins.
    // Real HTTP enrollment/Hello and normal session requests prove this adapter.
    // No original pre-fix red has been observed for this exact historical-piece scenario.
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let proof=async {
        let first=start(root.path(),&jwks).await?;
        let peers=async {
            let client=ConformanceClient::new(first.base_url())?;
            let a=enroll_admitted_peer(&client,FIRST,"019f0000-0000-7000-8000-000000001014","cutoff-origin-a","019f0000-0000-7000-8000-000000000fc1").await?;
            let b=enroll_admitted_peer(&client,SECOND,"019f0000-0000-7000-8000-000000001015","cutoff-origin-b","019f0000-0000-7000-8000-000000000fd1").await?;
            let c=enroll_admitted_peer(&client,CURRENT,"019f0000-0000-7000-8000-000000001016","cutoff-origin-c","019f0000-0000-7000-8000-000000000fe1").await?;
            let peers=(PeerId::new(a.peer)?,PeerId::new(b.peer)?,PeerId::new(c.peer)?);
            for admitted in [a,b,c] {let _=admitted.socket.close(CloseCode::Normal,None).await;}
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(peers)
        }.await;
        first.shutdown().await?;
        let (a,b,c)=peers?;
        let database=DatabaseId::new(Uuid::now_v7())?;
        history::register_grant_history(root.path(),configuration::registry_config()?,database,
            vec![(Epoch::new(1)?,a),(Epoch::new(2)?,b),(Epoch::new(3)?,c)]).await?;
        let server=start(root.path(),&jwks).await?;
        let proof=async {
            let mut first=connect(server.base_url(),"cutoff-origin-a","019f0000-0000-7000-8000-000000000fc2").await?;
            let proof=async {
                let end=ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(0));
                let piece=Piece::<BoardAnnotation>::new(PieceContents {
                    database,epoch:Epoch::new(1)?,continues_from:end,
                    through:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(1)),
                    transactions:vec![ReplicatedTransaction::Rows {commit_id:CommitSequence::new(1),rows:vec![]}],part:None,annotation:None,
                })?;
                let late=operation(&mut first,json!({"kind":"replication","value":{"kind":"piece","value":piece}})).await?;
                if !matches!(serde_json::from_value::<OperationAnswer<BoardConsumerProtocol>>(late.clone())?,
                    OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer,ended_at}))
                    if newer==Epoch::new(3)? && ended_at==end) {
                    return Err(format!("late piece did not preserve its old epoch's end: {late}").into());
                }
                Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
            }.await;
            for socket in [first] {let _=socket.close(CloseCode::Normal,None).await;}
            proof
        }.await;
        server.shutdown().await?;
        proof
    }.await;
    jwks.shutdown().await?;
    proof
}
