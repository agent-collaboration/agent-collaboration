//! Decode journey requests with the product's current Serde DTOs, without a Host.
//!
//! Usage: board-request-shapes < requests.json
//! Input is an array of {"decoder":"project", "request":{...}} envelopes.
//! Any rejected request exits nonzero; success reports only the decoded count.
use serde::Deserialize;
use std::io::{self, Read};

#[derive(Deserialize)]
#[serde(tag = "decoder", content = "request", rename_all = "camelCase")]
enum WireRequest {
    Project(Box<message_board::ProjectRequest>),
    ProjectCreate(Box<message_board::ProjectCreateRequest>),
    ThreadCreate(Box<message_board::ThreadCreateRequest>),
    ThreadPost(Box<message_board::ThreadPostRequest>),
    ThreadJoin(Box<message_board::ThreadJoinRequest>),
    ThreadWatch(Box<message_board::ThreadWatchRequest>),
    ThreadUnwatch(Box<message_board::ThreadUnwatchRequest>),
    ProjectWatch(Box<message_board::ProjectWatchRequest>),
    ThreadResolve(Box<message_board::ThreadResolveRequest>),
    ThreadRefile(Box<message_board::ThreadRefileRequest>),
    ThreadSubscribe(Box<collaboration_protocol::ThreadSubscribeRequest>),
    Acknowledgement(Box<message_board::AcknowledgementEntry>),
}

impl WireRequest {
    // Consume each decoded field so this apparatus retains neither requests nor state.
    fn discard(self) {
        match self {
            Self::Project(value) => drop(value),
            Self::ProjectCreate(value) => drop(value),
            Self::ThreadCreate(value) => drop(value),
            Self::ThreadPost(value) => drop(value),
            Self::ThreadJoin(value) => drop(value),
            Self::ThreadWatch(value) => drop(value),
            Self::ThreadUnwatch(value) => drop(value),
            Self::ProjectWatch(value) => drop(value),
            Self::ThreadResolve(value) => drop(value),
            Self::ThreadRefile(value) => drop(value),
            Self::ThreadSubscribe(value) => drop(value),
            Self::Acknowledgement(value) => drop(value),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--help") {
        println!(
            "board-request-shapes < requests.json\nDecode an array of decoder/request envelopes using the current product DTOs. No server, database or network; invalid input exits nonzero."
        );
        return Ok(());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let requests: Vec<WireRequest> = serde_json::from_str(&input)?;
    let count = requests.len();
    if count == 0 {
        return Err(io::Error::other("at least one request is required").into());
    }
    for request in requests {
        request.discard();
    }
    println!("{}", serde_json::json!({"decoded":count}));
    Ok(())
}
