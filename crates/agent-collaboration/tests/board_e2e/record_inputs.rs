//! Literal fact, task and trail inputs shared by BR07/08; no product rule implementation.
use super::fixture::{BoardFixture, accepted, at, identity};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub fn reference(kind: &str, id: &str) -> Value {
    json!({"kind":kind,"id":id})
}
pub fn evidence(kind: &str, id: &str) -> Value {
    json!({"kind":"record","record":reference(kind,id)})
}

pub async fn task(fixture: &BoardFixture, title: &str) -> FixtureResult<String> {
    let task = identity();
    let reply = fixture
        .cli(&[
            "task",
            "create",
            "--project-id",
            &fixture.project,
            "--task-id",
            &task,
            "--workstream-id",
            &fixture.workstream,
            "--title",
            title,
            "--actor",
            &fixture.owner.to_string(),
        ])
        .await?;
    accepted(at(&reply, "/record")?)?;
    Ok(task)
}

pub async fn trail(
    fixture: &BoardFixture,
    actor: &Value,
    subject: Value,
    tag: &str,
    text: &str,
    links: Vec<Value>,
) -> FixtureResult<String> {
    let row = identity();
    let mut arguments = vec![
        "work-trail".to_owned(),
        "add".to_owned(),
        text.to_owned(),
        "--project-id".to_owned(),
        fixture.project.clone(),
        "--row-id".to_owned(),
        row.clone(),
        "--subject".to_owned(),
        subject.to_string(),
        "--actor".to_owned(),
        actor.to_string(),
        "--tag".to_owned(),
        tag.to_owned(),
    ];
    for link in links {
        arguments.extend(["--link".to_owned(), link.to_string()]);
    }
    let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    accepted(at(&fixture.cli(&arguments).await?, "/record")?)?;
    Ok(row)
}

pub fn evaluator(fixture: &BoardFixture, anchor: Value) -> Value {
    fixture.request(&fixture.owner,json!({"kind":"factRecord","fact":{"factId":identity(),
        "kind":{"kind":"evaluatorResult","fact":{"guard":"proof","subject":reference("project",&fixture.project),
        "outcome":"met","evidence":[],"evaluator":{"actor":fixture.owner,"engine":null,"calibration":null},"executor":null}},
        "links":[reference("project",&fixture.project)],"producerTime":chrono::Utc::now(),"anchor":anchor}}))
}
