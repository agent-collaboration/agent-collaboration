//! A decoded remote answer or a call whose result was not received.

#[derive(Debug)]
pub enum CallOutcome<Reply> {
    Answered(Reply),
    Failed(durable_steps::CallFailure),
}
