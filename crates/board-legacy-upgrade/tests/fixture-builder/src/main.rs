//! Regenerates the role-history fixture through the actual retired BoardStore API.
use message_board_storage::BoardStore;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{error::Error, path::PathBuf};

type BuilderResult<T = ()> = Result<T, Box<dyn Error>>;
fn request<T: DeserializeOwned>(value: Value) -> BuilderResult<T> {
    Ok(serde_json::from_value(value)?)
}
fn identity(name: &str) -> Value {
    json!({"kind":"session","session":{"endpoint":{"serviceId":"550e8400-e29b-41d4-a716-446655440000","endpointId":"codex-local"},"sessionId":name}})
}
fn id(number: u64) -> String {
    format!("019a0000-0000-7000-8000-{number:012x}")
}

#[tokio::main]
async fn main() -> BuilderResult {
    let path = PathBuf::from(std::env::args().nth(1).ok_or("output path required")?);
    if path.exists() {
        return Err("refuse to overwrite fixture".into());
    }
    let at =
        chrono::DateTime::parse_from_rfc3339("2023-11-14T22:15:00Z")?.with_timezone(&chrono::Utc);
    let owner = json!({"kind":"human","humanId":"legacy-owner"});
    let mut store = BoardStore::open(&path).await?;
    store.create_project(request(json!({"projectId":id(1),"name":"Role history workspace","description":"","actor":owner,"actingFor":null}))?).await?;
    store.create_board(request(json!({"projectId":id(1),"boardId":id(11),"name":"Role history board","description":"","actor":owner,"actingFor":null}))?).await?;
    for topic in [21, 22] {
        store.create_topic(request(json!({"boardId":id(11),"topicId":id(topic),"name":format!("Project {topic}"),"description":format!("Preserved description {topic}"),"actor":owner,"actingFor":null}))?).await?;
    }
    store.create_thread(request(json!({"messageId":id(101),"topicId":id(21),"actor":identity("actor-a"),"role":"orchestrator","watch":false,"actingFor":null,"text":"Session root\nsecond line","references":[]}))?,at).await?;
    for role in ["advisor", "reviewer"] {
        store.join_thread(request(json!({"rootMessageId":id(101),"actor":identity("actor-b"),"role":role,"replace":null,"watch":false,"note":"Retained note"}))?,at).await?;
    }
    store.leave_thread(request(json!({"rootMessageId":id(101),"actor":identity("actor-a"),"to":identity("actor-b"),"resolve":false}))?,at).await?;
    store.create_thread(request(json!({"messageId":id(102),"topicId":id(21),"actor":owner,"role":"orchestrator","watch":false,"actingFor":null,"text":"Human root","references":[]}))?,at).await?;
    let case = std::env::args().nth(2);
    if case.as_deref() == Some("mixed-watch-scope") {
        store.create_thread(request(json!({"messageId":id(103),"topicId":id(21),"actor":identity("actor-c"),"role":"orchestrator","watch":false,"actingFor":null,"text":"Unwatched root","references":[]}))?,at).await?;
    }
    if matches!(case.as_deref(), Some("watch-scope" | "mixed-watch-scope")) {
        store
            .watch_topic(request(
                json!({"topicId":id(21),"actor":owner,"actingFor":null}),
            )?)
            .await?;
        if case.as_deref() == Some("mixed-watch-scope") {
            store
                .watch_thread(request(
                    json!({"rootMessageId":id(101),"actor":owner,"actingFor":null}),
                )?)
                .await?;
        }
        store.post_message(request(json!({"messageId":id(201),"placement":{"kind":"thread","rootMessageId":id(101)},"actor":identity("actor-b"),"actingFor":null,"text":"Reply after a topic watch","references":[]}))?,at).await?;
        if case.as_deref() == Some("mixed-watch-scope") {
            store.post_message(request(json!({"messageId":id(202),"placement":{"kind":"thread","rootMessageId":id(103)},"actor":identity("actor-c"),"actingFor":null,"text":"Reply in an unwatched thread","references":[]}))?,at).await?;
        }
    }
    store.close().await?;
    println!("legacy fixture created through legacy BoardStore ffced1c1");
    Ok(())
}
