use super::*;
use collaboration_protocol::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, MessageBody,
    MessageSender,
};

#[tokio::test]
async fn leave_cancels_the_fired_collated_message_but_not_direct_mail() {
    let f = Fixture::new().await;
    let area = f.area(7400).await;
    let thread = f.thread(7401, area).await;
    let (observer, recipient) = joined_observer(&f, &thread).await;
    created(f.call(7402, post(&thread, vec![])).await);
    let direct = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(7403),
                operation: CentralMailOperation::SendMessage {
                    to: session(&observer),
                    body: "direct remains queued".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued {
        message_id: direct_id,
    }) = direct
    else {
        panic!("direct send refused: {direct:?}")
    };
    let runtime = f.host.central(&f.caller.organization_id).await;
    f.clock.0.fetch_add(900_000, Ordering::SeqCst);
    // The observer's join also opened a window for the thread creator.
    assert_eq!(runtime.on_alarm().await.unwrap().windows_fired, 2);
    let query = || Statement {
        sql: "SELECT message_id,sender,body,state FROM messages WHERE recipient_actor=? AND source_thread_id=? AND message_kind='notice'",
        args: vec![text(&recipient), text(&thread)],
    };
    let before = runtime.read(query()).await.unwrap();
    assert_eq!(before.rows.len(), 1);
    let (SqlValue::Text(message_id), SqlValue::Text(sender), SqlValue::Text(body)) =
        (&before.rows[0][0], &before.rows[0][1], &before.rows[0][2])
    else {
        panic!("notice encoding")
    };
    let notice_id: workspace_ids::MessageId = message_id.parse().unwrap();
    assert_eq!(
        serde_json::from_str::<MessageSender>(sender).unwrap(),
        MessageSender::Central
    );
    assert!(
        matches!(serde_json::from_str::<MessageBody>(body).unwrap(), MessageBody::Notice { items } if items.len() == 1)
    );
    assert_eq!(before.rows[0][3], text("queued"));
    let left = call(
        &f.host,
        &observer,
        Some(f.workspace.clone()),
        7404,
        m::WorkspaceOperation::LeaveThread {
            thread_id: thread.clone(),
        },
    )
    .await;
    assert!(matches!(
        left.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let after = runtime.read(query()).await.unwrap();
    assert_eq!(after.rows.len(), 1);
    assert_eq!(after.rows[0][0], text(&notice_id));
    assert_eq!(&after.rows[0][1..3], &before.rows[0][1..3]);
    assert_eq!(after.rows[0][3], text("cancelled"));
    let direct = runtime
        .read(Statement {
            sql: "SELECT state FROM messages WHERE message_id=? AND message_kind='direct'",
            args: vec![text(&direct_id)],
        })
        .await
        .unwrap();
    assert_eq!(direct.rows, vec![vec![text("queued")]]);
    f.host.close().await;
}
