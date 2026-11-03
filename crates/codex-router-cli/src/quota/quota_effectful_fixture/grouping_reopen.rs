//! Two real CLI invocations share one owned state, but independently reopen their UI store.
use super::{
    FixtureResult, Scenario, effect_ports::AccountEffects, owned_state::OwnedQuotaState,
    receipts::FixtureEvents,
};
use agent_router_quota_tui::{
    QuotaGroupFacet,
    test_support::{QuotaFixtureReadback, QuotaFixtureView, with_quota_fixture_view},
};
use codex_router_quota_service::quota_reset::LoopbackInteractiveResetSessionFactory;
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::{io, path::PathBuf};

pub(super) struct GroupingReopenInputs<'a> {
    pub(super) state: &'a OwnedQuotaState,
    pub(super) fixture: QuotaFixtureView,
    pub(super) accounts: &'a AccountEffects,
    pub(super) events: FixtureEvents,
    pub(super) width: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupingInvocationFacts {
    invocation: u8,
    initial_mount: u64,
    final_mount: u64,
    #[serde(serialize_with = "serialize_facet")]
    initial_facet: QuotaGroupFacet,
    #[serde(serialize_with = "serialize_facet")]
    final_facet: QuotaGroupFacet,
    stored_value: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupingReopenFacts {
    store_path: PathBuf,
    invocations: Vec<GroupingInvocationFacts>,
}
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "Serde serialize_with requires a borrowed field"
)]
fn serialize_facet<T: Serializer>(
    facet: &QuotaGroupFacet,
    serializer: T,
) -> Result<T::Ok, T::Error> {
    serializer.serialize_str(facet.stored_value())
}

pub(super) fn observed_facet(readback: &QuotaFixtureReadback) -> FixtureResult<QuotaGroupFacet> {
    let selected = [
        ("quota:group-none", QuotaGroupFacet::None),
        ("quota:group-provider", QuotaGroupFacet::Provider),
        ("quota:group-status", QuotaGroupFacet::Status),
    ]
    .into_iter()
    .filter_map(|(id, facet)| {
        readback
            .actions
            .iter()
            .any(|action| action.id.as_str() == id && action.checked == Some(true))
            .then_some(facet)
    })
    .collect::<Vec<_>>();
    match selected.as_slice() {
        [facet] => Ok(*facet),
        _ => Err(io::Error::other(
            "actual published Browse must have exactly one checked grouping action",
        )
        .into()),
    }
}

pub(super) async fn run(
    inputs: GroupingReopenInputs<'_>,
) -> FixtureResult<(GroupingReopenFacts, Vec<Value>)> {
    let store_path = inputs.state.root().join("agent-proxy-ui.sqlite");
    if store_path.exists() {
        return Err(io::Error::other("grouping fixture must start without UI preferences").into());
    }
    let provider = super::loopback_provider::HeldLoopbackProvider::bind()?;
    let factory = LoopbackInteractiveResetSessionFactory::new(provider.address());
    let base_url = format!("http://{}", provider.address());
    let mut invocations = Vec::new();
    for invocation in [1u8, 2] {
        let mut publications = inputs.fixture.controller.subscribe();
        let run = inputs.state.with_account_commands(
            inputs.accounts.commands(
                inputs.state.root().to_owned(),
                Scenario::GroupingReopen,
                inputs.events.clone(),
            ),
            with_quota_fixture_view(
                inputs.fixture.clone(),
                crate::quota::quota_status_command::render_interactive_quota_status(
                    inputs.state.root().to_owned(),
                    Some(usize::from(inputs.width)),
                    false,
                    codex_router_quota_service::quota::current_unix_seconds(),
                    &factory,
                    &base_url,
                ),
            ),
        );
        tokio::pin!(run);
        let initial = tokio::select! {
            published=publications.recv()=>published.ok_or_else(||io::Error::other("grouping invocation ended before its first publication"))?,
            result=&mut run=>{result??;return Err(io::Error::other("grouping invocation returned without an initial published mount").into());}
        };
        let initial_facet = observed_facet(&initial)?;
        inputs.events.write(json!({"kind":"grouping-invocation-started","invocation":invocation,"mount":initial.mount,"facet":initial_facet.stored_value(),"storePath":store_path}))?;
        run.await??;
        if inputs.fixture.controller.readback().is_ok() {
            return Err(io::Error::other(
                "grouping invocation returned before actual view teardown",
            )
            .into());
        }
        let final_pose =
            inputs.fixture.controller.last_published().ok_or_else(|| {
                io::Error::other("grouping invocation has no final published pose")
            })?;
        let final_facet = observed_facet(&final_pose)?;
        let store = ui_preference_store::UiPreferenceStore::open_and_migrate(&store_path).await;
        let stored_value = store.load("quota", "group-by").await;
        store.close().await;
        let facts = GroupingInvocationFacts {
            invocation,
            initial_mount: initial.mount,
            final_mount: final_pose.mount,
            initial_facet,
            final_facet,
            stored_value,
        };
        inputs
            .events
            .write(json!({"kind":"grouping-invocation-settled","facts":facts}))?;
        invocations.push(facts);
    }
    let facts = GroupingReopenFacts {
        store_path,
        invocations,
    };
    validate(&serde_json::to_value(&facts)?)?;
    let requests=provider.finish()?.into_iter().map(|request|json!({"method":request.method,"path":request.path,"routingAccount":request.routing_account,"responseStatus":request.response_status})).collect();
    Ok((facts, requests))
}

pub(super) fn validate(report: &Value) -> FixtureResult<()> {
    let invocations = report["invocations"]
        .as_array()
        .ok_or_else(|| io::Error::other("actual grouping invocation facts are missing"))?;
    let [first, second] = invocations.as_slice() else {
        return Err(io::Error::other(
            "grouping-reopen requires exactly two settled real CLI invocations",
        )
        .into());
    };
    let first_mount = first["initialMount"].as_u64();
    if !report["storePath"].as_str().is_some_and(|path| {
        std::path::Path::new(path).is_absolute()
            && std::path::Path::new(path)
                .file_name()
                .is_some_and(|name| name == "agent-proxy-ui.sqlite")
    }) || first["invocation"] != 1
        || second["invocation"] != 2
        || !first_mount.is_some_and(|mount| mount > 0)
        || first["finalMount"] != first["initialMount"]
        || second["initialMount"].as_u64() != first_mount.and_then(|mount| mount.checked_add(1))
        || second["finalMount"] != second["initialMount"]
        || first["initialFacet"] != "none"
        || first["finalFacet"] != "provider"
        || first["storedValue"] != "provider"
        || second["initialFacet"] != "provider"
        || second["finalFacet"] != "status"
        || second["storedValue"] != "status"
    {
        return Err(io::Error::other("actual grouping store/facet/mount facts differ from None→Provider→reopen Provider→Status").into());
    }
    Ok(())
}
