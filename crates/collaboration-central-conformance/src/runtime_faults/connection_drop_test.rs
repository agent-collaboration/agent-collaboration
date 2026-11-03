//! A frame identity is scoped to its actual transport connection on the Fleet.

use super::{AnswerFaultObservation, ConnectionId, FrameId, RuntimeControl, RuntimeFaults};

#[tokio::test]
async fn an_answer_drop_keeps_another_connections_same_frame_and_remains_armed()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let selected = ConnectionId::new(durable_steps::Uuid::now_v7())?;
    let other = ConnectionId::new(durable_steps::Uuid::now_v7())?;
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let faults = RuntimeFaults::default();
    faults
        .control(RuntimeControl::DropNextAnswer {
            connection: selected,
            frame,
        })
        .await?;

    if faults.drop_answer(other, frame)? {
        return Err(
            "the selected connection consumed another connection's same-frame answer".into(),
        );
    }
    if !matches!(faults.observe()?.answer, AnswerFaultObservation::Armed {connection, frame: actual}
        if connection == selected && actual == frame)
    {
        return Err("the other connection consumed the selected drop".into());
    }

    if !faults.drop_answer(selected, frame)? || faults.drop_answer(selected, frame)? {
        return Err("the exact connection/frame must drop exactly once".into());
    }
    let observed = faults
        .control(RuntimeControl::AwaitDroppedAnswer {
            connection: selected,
            frame,
        })
        .await?;
    if !matches!(observed.answer, AnswerFaultObservation::Dropped {connection, frame: actual}
        if connection == selected && actual == frame)
    {
        return Err("the causal observation lost the exact connection/frame identity".into());
    }
    Ok(())
}
