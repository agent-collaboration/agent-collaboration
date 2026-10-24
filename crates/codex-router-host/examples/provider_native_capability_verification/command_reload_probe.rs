//! J3 replacement probe: make one real native command disappear and observe whether
//! the provider's successive full command lists replace it.
//!
//! The only change is the owned second fixture skill, removed after its bytes are
//! checked against the recorded materialization and restored to exactly those bytes
//! afterwards, even when a step fails. The provider's own `/reload-skills` arrives as
//! real native input through the public Router prompt; its operations are counted
//! against their own baseline. No list event is generated and no watcher is added. A
//! list without the removal is reported as unobserved, never as a pass.
use crate::conversation_oracle::{check_command_catalogues, new_operations};
use crate::journey_run::Journey;
use crate::native_fixture::{CANARY_PLACEHOLDER, fixture_source, write_private};
use crate::verification_models::FixtureRecord;
use serde::Serialize;
use std::{fs, path::Path, time::Duration};

const RELOAD_COMMAND: &str = "/reload-skills";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReloadProbe {
    pub(crate) removed_skill: String,
    pub(crate) reload_prompt_exit: Option<i32>,
    /// Router operations the deliberate reload added, against their own baseline.
    pub(crate) reload_operation_kinds: Vec<String>,
    pub(crate) catalogue_item_ids: Vec<String>,
    pub(crate) removed_names: Vec<String>,
    pub(crate) removal_observed: bool,
}

/// The exact bytes materialization wrote for the second fixture skill.
pub(crate) fn recorded_second_skill_bytes(
    repository: &Path,
    fixture: &FixtureRecord,
) -> Result<String, String> {
    let template = fixture_source(repository)
        .join(&fixture.second_skill_name)
        .join("SKILL.md");
    let body = fs::read_to_string(&template)
        .map_err(|error| format!("fixture template unreadable: {error}"))?;
    Ok(body.replace(CANARY_PLACEHOLDER, &fixture.second_canary))
}

pub(crate) async fn reload_after_fixture_removal(
    journey: &Journey,
    reply_bound: Duration,
) -> Result<ReloadProbe, String> {
    let fixture = &journey.target.fixture;
    let recorded = recorded_second_skill_bytes(&journey.target.plan.repository, fixture)?;
    let current = fs::read_to_string(&fixture.second_skill_file)
        .map_err(|error| format!("second fixture skill unreadable: {error}"))?;
    if current != recorded {
        return Err(
            "second fixture skill no longer holds its recorded bytes; not removed".to_owned(),
        );
    }
    let baseline = journey.operations("reload-baseline").await?;
    fs::remove_file(&fixture.second_skill_file)
        .map_err(|error| format!("second fixture skill not removed: {error}"))?;
    let reload = async {
        let exit = journey
            .router_prompt("reload-prompt", RELOAD_COMMAND, reply_bound)
            .await?;
        let reloaded = journey.operations("reload-after").await?;
        // The complete retained list history: the earlier lists hold the removed name.
        let retained = journey.observe("reload-after", None).await?;
        Ok::<_, String>((exit, reloaded, retained))
    }
    .await;
    let restored = write_private(&fixture.second_skill_file, recorded.as_bytes());
    let (exit, reloaded, retained) = match (reload, restored) {
        (reload, Err(error)) => {
            let earlier = reload
                .err()
                .map(|error| format!("; earlier: {error}"))
                .unwrap_or_default();
            return Err(format!(
                "second fixture skill not restored: {error}{earlier}"
            ));
        }
        (reload, Ok(())) => reload?,
    };
    let catalogues = check_command_catalogues(&retained, fixture)?;
    let removal_observed = catalogues
        .removed_names
        .contains(&fixture.second_skill_name);
    Ok(ReloadProbe {
        removed_skill: fixture.second_skill_name.clone(),
        reload_prompt_exit: exit,
        reload_operation_kinds: new_operations(&baseline, &reloaded)
            .iter()
            .map(|row| row.operation_kind.clone())
            .collect(),
        catalogue_item_ids: catalogues.catalogue_item_ids,
        removed_names: catalogues.removed_names,
        removal_observed,
    })
}
