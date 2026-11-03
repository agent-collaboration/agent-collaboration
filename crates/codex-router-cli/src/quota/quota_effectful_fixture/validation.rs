//! Completion is a guarded Quit plus the exact real effect set for this scenario.
use super::{FixtureResult, configuration::Scenario};
use serde_json::Value;
use std::io;
fn require(condition: bool, message: &str) -> FixtureResult<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.to_owned()).into())
    }
}
fn array<'a>(value: &'a Value, key: &str) -> FixtureResult<&'a Vec<Value>> {
    value[key]
        .as_array()
        .ok_or_else(|| io::Error::other(format!("missing actual {key} facts")).into())
}
pub(super) fn validate(scenario: Scenario, summary: &Value) -> FixtureResult<()> {
    let outcomes = array(summary, "outcomes")?;
    require(
        outcomes
            .last()
            .is_some_and(|outcome| outcome["kind"] == "quit"),
        "fixture did not terminate through the actual guarded Quit",
    )?;
    require(
        array(summary, "unexpectedEffectCalls")?.is_empty(),
        "fixture invoked an unrelated effect port",
    )?;
    require(
        array(summary, "resetRequests")?.is_empty(),
        "scenario made an unexpected reset provider request",
    )?;
    let native = array(summary, "nativeDispatch")?;
    require(
        !native.is_empty(),
        "fixture lacks actual InputOwner receipts",
    )?;
    require(
        native.iter().all(|receipt| {
            receipt["sequence"].is_u64()
                && receipt["event"].is_object()
                && receipt["surface"].is_object()
                && receipt.get("kind").is_none()
        }),
        "fixture setup was mixed into the native receipt schema",
    )?;
    require(
        native
            .iter()
            .filter(|receipt| receipt["action"] == "quota:refresh")
            .count()
            == usize::from(scenario.is_refresh()),
        "native input invoked an unexpected number of refresh actions",
    )?;
    require(
        native
            .iter()
            .filter(|receipt| receipt["action"] == "quota:login")
            .count()
            == usize::from(scenario.is_login()),
        "native input invoked an unexpected number of login actions",
    )?;
    let policies_after = &summary["policiesAfter"];
    require(
        array(policies_after, "floors")?.is_empty(),
        "scenario unexpectedly persisted a weekly-floor policy",
    )?;
    let credits_before = array(&summary["policiesBefore"], "credits")?;
    require(
        array(policies_after, "credits")?
            .iter()
            .all(|after| credits_before.iter().any(|before| before == after)),
        "scenario unexpectedly changed a credit policy",
    )?;
    require(
        native.iter().all(|receipt| {
            !matches!(
                receipt["action"].as_str(),
                Some(
                    "quota:save-credit"
                        | "quota:refresh-credit"
                        | "quota:floor-save"
                        | "quota:reset-advance"
                        | "quota:inspect-reset"
                )
            )
        }),
        "scenario accepted an unrelated policy or reset owner action",
    )?;
    if scenario == Scenario::GroupingReopen {
        super::grouping_reopen::validate(&summary["groupingReopen"])?;
        require(
            outcomes.len() == 2
                && outcomes.iter().all(|outcome| outcome["kind"] == "quit")
                && native
                    .iter()
                    .filter(|receipt| receipt["action"] == "quota:group-cycle")
                    .count()
                    == 2
                && native
                    .iter()
                    .filter(|receipt| receipt["action"] == "quota:quit")
                    .count()
                    == 2,
            "grouping-reopen requires two actual grouping activations and terminal Quits",
        )?;
    }
    validate_preference_writes(scenario, summary)?;
    let commands = array(summary, "accountCommands")?;

    let expected: &[&str] = match scenario {
        Scenario::CapturedActions => &["copy-id", "disable", "enable"],
        Scenario::RemoveMiddle
        | Scenario::RemoveLast
        | Scenario::RemoveOnly
        | Scenario::RemoveHeld
        | Scenario::RemoveRequiresDisabled => &["remove"],
        _ => &[],
    };
    require(
        commands.len() == expected.len(),
        "fixture account effect count differs from its declared scenario",
    )?;
    for (native_action, owner_actions) in [
        ("quota:copy-id", &["copy-id"][..]),
        ("quota:account-status", &["disable", "enable"][..]),
        ("quota:confirm-remove", &["remove"][..]),
    ] {
        require(
            native
                .iter()
                .filter(|receipt| receipt["action"] == native_action)
                .count()
                == commands
                    .iter()
                    .filter(|command| {
                        command["command"]["action"]
                            .as_str()
                            .is_some_and(|action| owner_actions.contains(&action))
                    })
                    .count(),
            "accepted native account action has no matching recorded owner command",
        )?;
    }
    let target = format!("effect-account-{}", scenario.captured_position());
    for (command, action) in commands.iter().zip(expected) {
        require(
            command["command"]["action"] == *action && command["command"]["accountId"] == target,
            "actual command retargeted or ran in the wrong sequence",
        )?;
        let result = &command["result"];
        if scenario == Scenario::RemoveRequiresDisabled {
            require(
                result["kind"] == "refused" && result["reason"] == "RequiresDisabled",
                "removal lost the real RequiresDisabled refusal",
            )?;
        } else {
            require(
                result["kind"] == "succeeded",
                "actual account owner did not succeed",
            )?;
        }
    }
    let copied = array(summary, "clipboard")?;
    require(
        if scenario == Scenario::CapturedActions {
            copied == &vec![Value::String(target.clone())]
        } else {
            copied.is_empty()
        },
        "clipboard effect differs from the full captured account ID",
    )?;
    let after = array(summary, "after")?;
    let target_present = after.iter().any(|row| row["accountId"] == target);
    let removed = matches!(
        scenario,
        Scenario::RemoveMiddle
            | Scenario::RemoveLast
            | Scenario::RemoveOnly
            | Scenario::RemoveHeld
            | Scenario::CapturedActions
    );
    require(
        target_present != removed,
        "actual final account presence differs from removal/disappearance outcome",
    )?;
    let mut expected_accounts = array(summary, "before")?.clone();
    if removed {
        expected_accounts.retain(|row| row["accountId"] != target);
    }
    for row in &mut expected_accounts {
        if row["accountId"] == target {
            if scenario == Scenario::RemoveRequiresDisabled {
                row["status"] = Value::String("enabled".to_owned());
            }
            if scenario == Scenario::LoginSucceeded {
                row["credentialGeneration"] = Value::from(8);
            }
        }
    }
    require(
        after == &expected_accounts,
        "actual account facts differ from the exact declared owner and setup effects",
    )?;
    let expected_mutations: &[&str] = match scenario {
        Scenario::CapturedActions => &["remove-captured-account-externally"],
        Scenario::RemoveRequiresDisabled => &["reenable-before-removal"],
        Scenario::LoginSucceeded => &["activate-fresh-login-generation"],
        _ => &[],
    };
    let mutations = array(summary, "fixtureMutations")?;
    require(
        mutations.len() == expected_mutations.len()
            && mutations
                .iter()
                .zip(expected_mutations)
                .all(|(mutation, expected)| mutation["operation"] == *expected),
        "fixture setup ledger differs from the declared scenario",
    )?;
    if scenario == Scenario::RemoveHeld {
        require(
            summary["removalGateEntered"] == true && summary["removalGateReleased"] == true,
            "held removal never reached and released the actual commit seam",
        )?;
    }
    let poses = array(summary, "publishedPoses")?;
    require(
        !poses.is_empty(),
        "fixture has no actual published component poses",
    )?;
    if scenario == Scenario::CapturedActions {
        require(
            array(summary, "fixtureMutations")?.len() == 1,
            "external disappearance setup was not completed exactly once",
        )?;
        for kind in ["details", "options", "floor"] {
            require(
                poses.iter().any(|pose| {
                    pose["captured"]["kind"] == kind
                        && pose["captured"]["accountId"] == target
                        && pose["browseAccountId"] != target
                        && pose["browseAccountId"].is_string()
                }),
                "captured A / Browse B proof is missing for an actual dialog",
            )?;
        }
    }
    if matches!(
        scenario,
        Scenario::RemoveMiddle | Scenario::RemoveLast | Scenario::RemoveOnly
    ) {
        let expected = after
            .get(scenario.captured_position())
            .or_else(|| after.last())
            .map(|row| row["accountId"].clone())
            .unwrap_or(Value::Null);
        require(
            poses.last().unwrap()["listSelection"] == expected,
            "actual list did not reselect the account at the removed position or last survivor",
        )?;
    }
    if scenario.is_login() {
        require(
            outcomes.len() == 2
                && outcomes[0]["kind"] == "login-again"
                && outcomes[0]["accountId"] == target,
            "real CLI restart loop did not retain one captured login then Quit",
        )?;
        require(
            summary["loginChild"]["exitCode"].as_u64() == scenario.login_exit().map(u64::from),
            "declared child exit is missing or differs",
        )?;
        validate_login_parent_exit(scenario.login_exit().unwrap(), &summary["loginParentExit"])?;
        require(
            poses.iter().any(|pose| {
                pose["captured"]["kind"] == "details"
                    && pose["captured"]["accountId"] == target
                    && pose["browseAccountId"] != target
            }),
            "login was not exercised with separately selected Browse B",
        )?;
        if scenario == Scenario::LoginSucceeded {
            require(
                after
                    .iter()
                    .any(|row| row["accountId"] == target && row["credentialGeneration"] == 8),
                "successful login did not activate a genuinely fresh fixture generation",
            )?;
            require(
                poses.last().unwrap()["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["accountId"] == target && row["loginState"] == "NotRequired"),
                "actual restarted report still requires login",
            )?;
        }
    } else {
        require(
            summary["loginChild"].is_null(),
            "non-login scenario ran a child",
        )?;
    }
    if scenario.is_refresh() {
        let refresh = &summary["refresh"];
        require(
            refresh["serviceCalls"] == 1
                && refresh["callsWhileHeld"] == 1
                && refresh["released"] == true,
            "refresh did not settle exactly one causally released invocation",
        )?;
        let observations = array(refresh, "observations")?;
        let source = observations
            .iter()
            .find(|event| event["kind"] == "service-returned" || event["kind"] == "setup-failed")
            .ok_or_else(|| io::Error::other("actual refresh source return is absent"))?;
        let mapped = observations
            .iter()
            .find(|event| event["kind"] == "mapping-returned")
            .ok_or_else(|| io::Error::other("actual refresh mapping return is absent"))?;
        let (source_kind, result_kind) = match scenario {
            Scenario::RefreshAllSuccess => ("ok", "refreshed"),
            Scenario::RefreshPartial => ("ok", "partial"),
            Scenario::RefreshAllFailed => ("all-failed", "failed"),
            _ => ("setup-failed", "failed"),
        };
        require(
            if source_kind == "setup-failed" {
                source["kind"] == source_kind
            } else {
                source["source"]["kind"] == source_kind
            },
            "fixture actual service outcome differs from declared refresh case",
        )?;
        require(
            mapped["result"]["kind"] == result_kind,
            "fixture actual mapper outcome differs from declared refresh case",
        )?;
        require(
            array(refresh, "requests")?
                .iter()
                .all(|request| request["method"] == "GET"),
            "refresh emitted an unexpected POST or other request",
        )?;
    } else {
        require(
            summary["refresh"].is_null(),
            "non-refresh scenario invoked the refresh fixture",
        )?;
    }
    Ok(())
}

pub(super) fn validate_login_parent_exit(expected: u8, parent: &Value) -> FixtureResult<()> {
    require(
        parent["childPid"].as_u64().is_some_and(|pid| pid > 0)
            && parent["waitOrdinal"] == 1
            && parent["status"]["kind"] == "exited"
            && parent["status"]["code"].as_u64() == Some(u64::from(expected))
            && parent["status"]["rawStatus"].as_i64() == Some(i64::from(expected) << 8),
        "actual parent wait identity/count/exit is missing or differs",
    )
}

pub(super) fn validate_preference_writes(scenario: Scenario, summary: &Value) -> FixtureResult<()> {
    let native = array(summary, "nativeDispatch")?;
    let writes = array(summary, "preferenceWrites")?;
    let group_cycles = native
        .iter()
        .filter(|receipt| receipt["action"] == "quota:group-cycle")
        .count();
    let expected: &[&str] = match (scenario, group_cycles) {
        (Scenario::GroupingReopen, 2) => &["provider", "status"],
        (Scenario::LoginSucceeded, 1) => &["provider"],
        (_, 0) => &[],
        _ => return Err(io::Error::other("undeclared preference proof route").into()),
    };
    require(
        writes.len() == expected.len()
            && writes
                .iter()
                .zip(expected)
                .enumerate()
                .all(|(index, (write, facet))| {
                    write["ordinal"].as_u64() == u64::try_from(index + 1).ok()
                        && write["facet"] == *facet
                        && write["result"]["kind"] == "saved"
                }),
        "actual preference save count/order/result differs from the accepted native choice",
    )?;
    require(
        summary["storedGrouping"].as_str() == expected.last().copied(),
        "actual final preference store differs from accepted native choice",
    )?;
    if scenario == Scenario::LoginSucceeded && group_cycles == 1 {
        let poses = array(summary, "publishedPoses")?;
        for mount in [1u64, 2] {
            require(
                poses.iter().any(|pose| {
                    pose["mount"].as_u64() == Some(mount)
                        && pose["actions"].as_array().is_some_and(|actions| {
                            actions.iter().any(|action| {
                                action["id"] == "quota:group-provider" && action["checked"] == true
                            })
                        })
                }),
                "repair profile lacks actual provider choice before login or on its fresh mount",
            )?;
        }
    }
    Ok(())
}
