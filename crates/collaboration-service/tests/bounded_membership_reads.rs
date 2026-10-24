//! Per-write reconciliation stays local; reader-wide discovery probes other stores once.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost, project_statements};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug)]
struct MembershipScopeCost {
    write_cost: ReadCost,
    unrelated_write_statements: u64,
    written_project_statements: u64,
    unrelated_reader_probes: Vec<u64>,
}
async fn scope_cost(other_projects: usize) -> Result<MembershipScopeCost, TestError> {
    let fixture = BoardFixture::start_poll().await?;
    let (project, workstream) = fixture.project().await?;
    let mut others = Vec::new();
    for index in 0..other_projects {
        let other = ProjectId::generate();
        let mut request = fixture.header();
        request.as_object_mut().ok_or("request")?.extend(
            json!({"boardId":fixture.board,"projectId":other,"title":format!("other {index}")})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        fixture
            .prepare_registered_creation(&serde_json::from_value(request.clone())?)
            .await?;
        accepted(&fixture.api.call("project_create", request).await?);
        others.push(other);
    }
    fixture.quiesce_mail_publication().await?;
    let before_others: u64 = others
        .iter()
        .map(|project| project_statements(project.as_str()))
        .sum();
    let before_own = project_statements(project.as_str());
    let before_cost = current_cost();
    accepted(&fixture.api.call("workstream_rename", fixture.request(&project, json!({"kind":"workstreamRename","workstreamId":workstream,"name":"changed","expectedRevision":0}))).await?);
    fixture
        ._delivery
        .as_ref()
        .ok_or("delivery")?
        .shutdown()
        .await;
    let write_cost = current_cost().since(before_cost);
    let unrelated_write_statements = others
        .iter()
        .map(|project| project_statements(project.as_str()))
        .sum::<u64>()
        - before_others;
    let written_project_statements = project_statements(project.as_str()) - before_own;
    let before_probes: Vec<_> = others
        .iter()
        .map(|project| project_statements(project.as_str()))
        .collect();
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(
            &fixture._directory.path().join("membership-costs.sqlite"),
        )
        .await?,
    );
    let windows = collaboration_service::SubscriptionWindowStore::new(
        fixture._registry.clone(),
        interactions,
    );
    let reader = Identity::Human {
        human_id: HumanId::try_from("absent-reader".to_owned())?,
    };
    let records = windows
        .list_reader_subscriptions(&reader, chrono::Utc::now())
        .await?;
    if !records.records.is_empty() || !records.unavailable.is_empty() {
        return Err("unexpected reader memberships".into());
    }
    let unrelated_reader_probes = others
        .iter()
        .zip(before_probes)
        .map(|(project, before)| project_statements(project.as_str()) - before)
        .collect();
    fixture.stop().await?;
    Ok(MembershipScopeCost {
        write_cost,
        unrelated_write_statements,
        written_project_statements,
        unrelated_reader_probes,
    })
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn writes_reconcile_only_their_project_and_reader_wide_reads_probe_once()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = scope_cost(1).await?;
    let large = scope_cost(8).await?;
    eprintln!("membership scope costs: small={small:?}, large={large:?}");
    assert_eq!(
        small.write_cost, large.write_cost,
        "other projects changed the write cost"
    );
    assert_eq!(
        small.written_project_statements,
        large.written_project_statements
    );
    for cost in [small, large] {
        assert_eq!(
            cost.unrelated_write_statements, 0,
            "write reconciliation read another project"
        );
        assert!(cost.written_project_statements > 0);
        assert!(cost.write_cost.statements > 0 && cost.write_cost.records > 0);
        assert!(
            cost.unrelated_reader_probes
                .iter()
                .all(|statements| *statements == 1),
            "reader-wide discovery read beyond its indexed probe"
        );
    }
    Ok(())
}
