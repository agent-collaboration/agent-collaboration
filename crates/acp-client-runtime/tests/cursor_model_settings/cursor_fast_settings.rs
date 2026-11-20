//! Cursor's fast option through the real ACP runtime against the reactive
//! fixture, which starts fast on and turns it back on at every model change
//! (D1 spec R7-R11).

#![expect(
    clippy::panic_in_result_fn,
    clippy::indexing_slicing,
    reason = "Wire-receipt assertions index the recorded requests and assert inside Result-returning tests."
)]

use super::acp_wire_fixture::AcpWireFixture;
use acp_client_runtime::{
    ExternalProviderRuntimeError, FastObserved, FastRequest, ProviderSettingRequest,
    RequestedProviderSettings,
};
use provider_session_machines::ProviderSettingKind;
use serde_json::{Value, json};
use session_event_model::{InputId, ProviderSettingFailureReason};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SESSION: &str = "reactive-session-1";

/// The create every test starts with: mode, model and effort named, and the
/// given fast request.
fn settings(fast: FastRequest) -> RequestedProviderSettings {
    RequestedProviderSettings {
        mode: Some("ask".to_owned()),
        model: Some("fixture-b".to_owned()),
        effort: Some("high".to_owned()),
        fast,
    }
}

async fn create(
    fixture: &AcpWireFixture,
    requested: RequestedProviderSettings,
) -> Result<acp_client_runtime::ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
    fixture
        .client
        .create_session_with_settings(fixture.root.path().into(), None, requested)
        .await
}

async fn set(
    fixture: &AcpWireFixture,
    kind: ProviderSettingKind,
    value: &str,
) -> Result<acp_client_runtime::EffectiveProviderSettings, ExternalProviderRuntimeError> {
    fixture
        .client
        .set_setting(
            SESSION.to_owned(),
            ProviderSettingRequest {
                kind,
                value: value.to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
}

async fn prompt(fixture: &AcpWireFixture) -> Result<(), ExternalProviderRuntimeError> {
    fixture
        .client
        .submit_with_approval_dispatch_for_input(
            SESSION.to_owned(),
            InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(vec![
                    session_event_model::PromptContent::text("hello".to_owned())
                        .map_err(|_| ExternalProviderRuntimeError::TransportFailure)?,
                ]),
            ),
            (),
            None,
        )
        .await
        .map(|_| ())
}

fn catalog_fast(fixture: &AcpWireFixture) -> Option<FastObserved> {
    fixture
        .client
        .settings_catalog(SESSION)
        .map(|catalog| catalog.fast_observed())
}

/// The configId and value of every set_config_option the fixture received.
fn setting_requests(fixture: &AcpWireFixture) -> Vec<(String, Value)> {
    fixture
        .requests()
        .into_iter()
        .filter(|request| request["method"] == "session/set_config_option")
        .map(|request| {
            (
                request["params"]["configId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                request["params"]["value"].clone(),
            )
        })
        .collect()
}

fn prompts_seen(fixture: &AcpWireFixture) -> usize {
    fixture
        .requests()
        .iter()
        .filter(|request| request["method"] == "session/prompt")
        .count()
}

fn failed_setting(
    result: Result<
        acp_client_runtime::ExternalProviderCreatedSession,
        ExternalProviderRuntimeError,
    >,
) -> Result<acp_client_runtime::FailedProviderSetting, Box<dyn std::error::Error>> {
    match result {
        Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
            failed, applied, ..
        }) => {
            let kinds: Vec<_> = applied.iter().map(|setting| setting.kind).collect();
            if failed.kind == ProviderSettingKind::Fast
                && kinds
                    != [
                        ProviderSettingKind::Mode,
                        ProviderSettingKind::Model,
                        ProviderSettingKind::Effort,
                    ]
            {
                return Err(format!("mode, model and effort should be applied: {kinds:?}").into());
            }
            Ok(failed)
        }
        other => Err(format!("expected a start created without settings, got {other:?}").into()),
    }
}

fn fast(value: &str) -> (String, Value) {
    ("fast".to_owned(), json!(value))
}

fn named(config_id: &str, value: &str) -> (String, Value) {
    (config_id.to_owned(), json!(value))
}

/// Oracle: D1 V11, spec R7 and R8 — with no --fast the Router turns fast
/// off, last, after mode, model and effort, and reports fast: off.
#[tokio::test]
async fn create_without_fast_selects_false_after_model() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    let created = create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    fixture.client.shutdown().await;
    assert_eq!(
        setting_requests(&fixture),
        vec![
            named("mode", "ask"),
            named("model", "fixture-b"),
            named("effort", "high"),
            fast("false")
        ]
    );
    assert_eq!(created.effective_settings.fast, Some(FastObserved::Off));
    Ok(())
}

/// The Router sends the Claude-shaped select's own off identifier, reads the
/// returned catalog as off, and dispatches exactly one subsequent prompt.
#[tokio::test]
async fn create_sends_advertised_fast_off_and_dispatches_one_prompt() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["on-off-fast"]).await;
    let created = create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    let prompted = prompt(&fixture).await;
    fixture.client.shutdown().await;

    assert_eq!(
        setting_requests(&fixture),
        vec![
            named("mode", "ask"),
            named("model", "fixture-b"),
            named("effort", "high"),
            fast("off")
        ]
    );
    assert_eq!(created.effective_settings.fast, Some(FastObserved::Off));
    prompted?;
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}

/// Oracle: D1 V11, spec R7 — --fast auto never touches fast, so the agent's
/// own default (on) is reported.
#[tokio::test]
async fn create_with_fast_auto_never_touches_fast() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    let created = create(&fixture, settings(FastRequest::Leave)).await?;
    fixture.client.shutdown().await;
    assert!(
        setting_requests(&fixture)
            .iter()
            .all(|(id, _)| id != "fast")
    );
    assert_eq!(created.effective_settings.fast, Some(FastObserved::On));
    Ok(())
}

/// A definite refusal is reported on create, keeps the observed fast value,
/// and permits one prompt without another setting operation.
#[tokio::test]
async fn refused_fast_off_is_reported_and_dispatches_one_prompt() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["on-off-fast", "refuse-fast-once"]).await;
    let failed = failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let observed = catalog_fast(&fixture);
    let allowed = prompt(&fixture).await;
    fixture.client.shutdown().await;

    assert_eq!(failed.kind, ProviderSettingKind::Fast);
    assert_eq!(failed.value, "off");
    assert!(failed.reason.as_str().contains("ACP code -32000"));
    assert_eq!(observed, Some(FastObserved::On));
    assert_eq!(setting_requests(&fixture).last(), Some(&fast("off")));
    allowed?;
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}

/// Accepting a reported fast refusal ends the keep-off policy, so a later
/// model change sends no fast mutation.
#[tokio::test]
async fn accepting_a_refused_fast_stops_keeping_it_off() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["refuse-fast-once"]).await;
    failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await?;
    let released = prompt(&fixture).await;
    let before_model = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Model, "fixture-a").await?;
    let after_model = setting_requests(&fixture);
    fixture.client.shutdown().await;
    released?;
    assert_eq!(after_model[before_model..], [named("model", "fixture-a")]);
    Ok(())
}

/// Oracle: D1 V14, spec R8 "Lost or unreadable" and Held — fast unknown, held,
/// accept refused; a confirmed off releases prompts.
#[tokio::test]
async fn lost_fast_off_answer_is_unknown_and_cannot_be_accepted() -> TestResult {
    let fixture =
        AcpWireFixture::start_reactive(&["malformed-fast-reply", "refuse-second-fast"]).await;
    let failed = failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let held = prompt(&fixture).await;
    let accepted = fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await;
    let refused_repair = set(&fixture, ProviderSettingKind::Fast, "off").await;
    let held_after_refusal = prompt(&fixture).await;
    let accepted_after_refusal = fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await;
    let prompts_before_confirmation = prompts_seen(&fixture);
    set(&fixture, ProviderSettingKind::Fast, "off").await?;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    assert_eq!(failed.kind, ProviderSettingKind::Fast);
    assert_eq!(failed.reason, ProviderSettingFailureReason::OutcomeUnknown);
    assert!(
        matches!(
            held,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "{held:?}"
    );
    assert!(
        matches!(
            accepted,
            Err(ExternalProviderRuntimeError::SettingsOutcomeUncertain)
        ),
        "{accepted:?}"
    );
    assert!(
        matches!(
            refused_repair,
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: ProviderSettingKind::Fast,
                ..
            })
        ),
        "{refused_repair:?}"
    );
    assert!(
        matches!(
            held_after_refusal,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "a refused repair cleared the earlier unknown hold: {held_after_refusal:?}"
    );
    assert!(
        matches!(
            accepted_after_refusal,
            Err(ExternalProviderRuntimeError::SettingsOutcomeUncertain)
        ),
        "a refused repair allowed Accept: {accepted_after_refusal:?}"
    );
    assert_eq!(prompts_before_confirmation, 0);
    assert_eq!(prompts_seen(&fixture), 1);
    assert_eq!(
        setting_requests(&fixture)
            .iter()
            .filter(|(id, _)| id == "fast")
            .count(),
        3
    );
    released?;
    Ok(())
}

/// Oracle: D1 V14 (Verifier challenge 2), spec R8 "the answer still reads on"
/// — a valid reply that still reads on is a definite refusal. The catalog
/// keeps reading on and the next prompt proceeds.
#[tokio::test]
async fn fast_off_reply_still_on_is_refused_without_holding() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["fast-stays-on"]).await;
    let failed = failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let allowed = prompt(&fixture).await;
    let observed = catalog_fast(&fixture);
    fixture.client.shutdown().await;
    assert_eq!(failed.kind, ProviderSettingKind::Fast);
    assert_ne!(failed.reason, ProviderSettingFailureReason::OutcomeUnknown);
    allowed?;
    assert_eq!(observed, Some(FastObserved::On));
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}

/// Oracle: D1 V14 (Verifier challenge 2), spec R8 "the answer no longer lists
/// a fast option" — a definite refusal. The returned catalog shows the absent
/// option and the next prompt proceeds.
#[tokio::test]
async fn fast_off_reply_without_fast_option_is_refused_without_holding() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["fast-option-vanishes"]).await;
    let failed = failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let allowed = prompt(&fixture).await;
    let observed = catalog_fast(&fixture);
    fixture.client.shutdown().await;
    assert_eq!(failed.kind, ProviderSettingKind::Fast);
    assert_ne!(failed.reason, ProviderSettingFailureReason::OutcomeUnknown);
    allowed?;
    // The reported catalog is the one the agent returned, never the stale one (V14).
    assert_eq!(observed, Some(FastObserved::NoOption));
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}

/// Two model changes, each followed at once by fast off, reading off after each.
async fn assert_two_model_changes_keep_fast_off(fixture: &AcpWireFixture) -> TestResult {
    for model in ["fixture-a", "fixture-b"] {
        let before = setting_requests(fixture).len();
        let effective = set(fixture, ProviderSettingKind::Model, model).await?;
        assert_eq!(
            setting_requests(fixture)[before..],
            [named("model", model), fast("false")]
        );
        assert_eq!(effective.fast, Some(FastObserved::Off));
        assert_eq!(effective.model.as_deref(), Some(model));
        assert_eq!(catalog_fast(fixture), Some(FastObserved::Off));
    }
    Ok(())
}

/// Oracle: D1 V15, spec R9 — the create default keeps fast off across every
/// model change, not just the first.
#[tokio::test]
async fn default_fast_off_survives_two_model_changes() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    let result = assert_two_model_changes_keep_fast_off(&fixture).await;
    fixture.client.shutdown().await;
    result
}

/// Oracle: D1 V15, spec R7 and R9 — created with --fast auto, then an
/// explicit fast off keeps fast off across model changes.
#[tokio::test]
async fn explicit_fast_off_survives_two_model_changes() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    create(&fixture, settings(FastRequest::Leave)).await?;
    let effective = set(&fixture, ProviderSettingKind::Fast, "off").await?;
    assert_eq!(effective.fast, Some(FastObserved::Off));
    let result = assert_two_model_changes_keep_fast_off(&fixture).await;
    fixture.client.shutdown().await;
    result
}

/// Oracle: D1 V15, spec R7 — fast auto sends nothing and stops keeping fast
/// off, so the next model change sends no fast set.
#[tokio::test]
async fn fast_auto_stops_keeping_fast_off() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    let before = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Fast, "auto").await?;
    let after_auto = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Model, "fixture-a").await?;
    let after_model = setting_requests(&fixture);
    fixture.client.shutdown().await;
    assert_eq!(after_auto, before, "fast auto sends nothing");
    assert_eq!(after_model[after_auto..], [named("model", "fixture-a")]);
    Ok(())
}

/// Oracle: D1 V15, spec R9 — a confirmed model followed by a refused fast off:
/// the error names fast and off, the new model stays, prompts proceed, and
/// keep-off still applies to later model changes.
#[tokio::test]
async fn model_change_with_refused_fast_off_keeps_input_available() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["refuse-fast-after-model"]).await;
    create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    let refused = set(&fixture, ProviderSettingKind::Model, "fixture-a").await;
    let model = fixture
        .client
        .settings_catalog(SESSION)
        .and_then(|catalog| catalog.effective_settings().model);
    let allowed = prompt(&fixture).await;
    set(&fixture, ProviderSettingKind::Fast, "off").await?;
    let released = prompt(&fixture).await;
    let before = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Model, "fixture-b").await?;
    let after = setting_requests(&fixture);
    fixture.client.shutdown().await;
    match refused {
        Err(ExternalProviderRuntimeError::SettingFailed { setting, value, .. }) => {
            assert_eq!(setting, ProviderSettingKind::Fast);
            assert_eq!(value, "off");
        }
        other => return Err(format!("expected a refused fast off, got {other:?}").into()),
    }
    assert_eq!(model.as_deref(), Some("fixture-a"));
    allowed?;
    released?;
    assert_eq!(
        after[before..],
        [named("model", "fixture-b"), fast("false")]
    );
    assert_eq!(prompts_seen(&fixture), 2);
    Ok(())
}

/// A retained keep-off policy has nothing to send when the agent's next
/// model does not offer fast. The model applies and input remains available.
#[tokio::test]
async fn model_change_reports_fast_not_offered_without_holding_input() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["fast-not-offered-on-fixture-b"]).await;
    create(
        &fixture,
        RequestedProviderSettings {
            model: Some("fixture-a".to_owned()),
            ..settings(FastRequest::OffIfOffered)
        },
    )
    .await?;
    let before_model = setting_requests(&fixture).len();
    let failure = set(&fixture, ProviderSettingKind::Model, "fixture-b").await;
    let catalog = fixture
        .client
        .settings_catalog(SESSION)
        .ok_or("settings catalog missing")?;
    let allowed = prompt(&fixture).await;
    fixture.client.shutdown().await;

    assert!(
        matches!(&failure, Err(ExternalProviderRuntimeError::FastNotOffered)),
        "{failure:?}"
    );
    assert_eq!(
        catalog.effective_settings().model.as_deref(),
        Some("fixture-b")
    );
    assert_eq!(catalog.fast_observed(), FastObserved::NoOption);
    assert_eq!(
        setting_requests(&fixture)[before_model..],
        [named("model", "fixture-b")]
    );
    allowed?;
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}

/// Oracle: D1 V15 (Verifier challenge 3), spec R9 and Held — a confirmed
/// model followed by a lost fast answer: unknown fast off, new model shown,
/// held, accept refused, and only a confirmed off releases it.
#[tokio::test]
async fn model_ok_then_fast_reply_lost_is_unknown_and_needs_a_confirmed_off() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["malformed-fast-after-model"]).await;
    create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    let lost = set(&fixture, ProviderSettingKind::Model, "fixture-a").await;
    let model = fixture
        .client
        .settings_catalog(SESSION)
        .and_then(|catalog| catalog.effective_settings().model);
    let held = prompt(&fixture).await;
    let accepted = fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await;
    set(&fixture, ProviderSettingKind::Fast, "off").await?;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    match lost {
        Err(ExternalProviderRuntimeError::SettingOutcomeUnknown { setting, value, .. }) => {
            assert_eq!(setting, ProviderSettingKind::Fast);
            assert_eq!(value, "off");
        }
        other => return Err(format!("expected an unknown fast off, got {other:?}").into()),
    }
    assert_eq!(model.as_deref(), Some("fixture-a"));
    assert!(
        matches!(
            held,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "{held:?}"
    );
    assert!(
        matches!(
            accepted,
            Err(ExternalProviderRuntimeError::SettingsOutcomeUncertain)
        ),
        "{accepted:?}"
    );
    released?;
    Ok(())
}

/// Fast auto on a held session: zero sets, still held, and the next model
/// change sends no fast.
async fn assert_fast_auto_keeps_the_hold(fixture: &AcpWireFixture) -> TestResult {
    let before = setting_requests(fixture).len();
    set(fixture, ProviderSettingKind::Fast, "auto").await?;
    assert_eq!(
        setting_requests(fixture).len(),
        before,
        "fast auto sends nothing"
    );
    let held = prompt(fixture).await;
    assert!(
        matches!(
            held,
            Err(ExternalProviderRuntimeError::SettingsUnresolved { .. })
        ),
        "{held:?}"
    );
    Ok(())
}

/// Fast auto after a definite refusal sends nothing and ends the keep-off
/// policy. The reported refusal remains available to explicit Accept.
#[tokio::test]
async fn fast_auto_after_a_refused_fast_change_sends_nothing() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["refuse-fast-once"]).await;
    failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let before_auto = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Fast, "auto").await?;
    assert_eq!(setting_requests(&fixture).len(), before_auto);
    let allowed_before_accept = prompt(&fixture).await;
    let before = setting_requests(&fixture).len();
    let model = set(&fixture, ProviderSettingKind::Model, "fixture-a").await;
    let after = setting_requests(&fixture);
    let accepted = fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    allowed_before_accept?;
    model?;
    assert_eq!(after[before..], [named("model", "fixture-a")]);
    accepted?;
    released?;
    assert_eq!(prompts_seen(&fixture), 2);
    Ok(())
}

/// Oracle: D1 V15b, spec R7 and Held — fast auto never counts as confirming a
/// lost fast answer; accept stays refused, and a confirmed off releases it.
#[tokio::test]
async fn fast_auto_keeps_an_unknown_fast_hold() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["malformed-fast-reply"]).await;
    failed_setting(create(&fixture, settings(FastRequest::OffIfOffered)).await)?;
    let kept = assert_fast_auto_keeps_the_hold(&fixture).await;
    let before = setting_requests(&fixture).len();
    let model = set(&fixture, ProviderSettingKind::Model, "fixture-a").await;
    let after = setting_requests(&fixture);
    let accepted = fixture
        .client
        .accept_session_settings(SESSION.to_owned())
        .await;
    set(&fixture, ProviderSettingKind::Fast, "off").await?;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    kept?;
    model?;
    assert_eq!(after[before..], [named("model", "fixture-a")]);
    assert!(
        matches!(
            accepted,
            Err(ExternalProviderRuntimeError::SettingsOutcomeUncertain)
        ),
        "{accepted:?}"
    );
    released?;
    Ok(())
}

/// A known model refusal leaves the last observed model and input available
/// before and after an explicit fast-auto policy change.
#[tokio::test]
async fn an_unoffered_model_is_reported_without_holding_input() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&[]).await;
    let failed = failed_setting(
        create(
            &fixture,
            RequestedProviderSettings {
                model: Some("missing-model".to_owned()),
                effort: None,
                ..settings(FastRequest::Leave)
            },
        )
        .await,
    )?;
    let before_auto = prompt(&fixture).await;
    let observed_model = fixture
        .client
        .settings_catalog(SESSION)
        .and_then(|catalog| catalog.effective_settings().model);
    let settings_before = setting_requests(&fixture).len();
    set(&fixture, ProviderSettingKind::Fast, "auto").await?;
    assert_eq!(setting_requests(&fixture).len(), settings_before);
    let after_auto = prompt(&fixture).await;
    set(&fixture, ProviderSettingKind::Model, "fixture-a").await?;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    assert_eq!(failed.kind, ProviderSettingKind::Model);
    assert_eq!(failed.reason, ProviderSettingFailureReason::InvalidSetting);
    assert_eq!(observed_model.as_deref(), Some("fixture-a"));
    before_auto?;
    after_auto?;
    released?;
    assert_eq!(prompts_seen(&fixture), 3);
    Ok(())
}

/// Oracle: D1 V16, spec R8 — a boolean fast option is set with a boolean.
#[tokio::test]
async fn boolean_fast_option_is_set_with_a_boolean_value() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["boolean-fast"]).await;
    let created = create(&fixture, settings(FastRequest::OffIfOffered)).await?;
    fixture.client.shutdown().await;
    let fast_request = fixture
        .requests()
        .into_iter()
        .find(|request| request["params"]["configId"] == "fast")
        .ok_or("no fast set")?;
    assert_eq!(
        fast_request["params"],
        json!({"sessionId":SESSION,"configId":"fast","type":"boolean","value":false})
    );
    assert_eq!(created.effective_settings.fast, Some(FastObserved::Off));
    Ok(())
}

/// An explicit fast off refusal is reported and leaves input available.
/// A later successful off still changes the observed value.
#[tokio::test]
async fn explicit_fast_off_refused_after_fast_auto_leaves_input_available() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["refuse-fast-once"]).await;
    create(&fixture, settings(FastRequest::Leave)).await?;
    let refused = set(&fixture, ProviderSettingKind::Fast, "off").await;
    let allowed = prompt(&fixture).await;
    set(&fixture, ProviderSettingKind::Fast, "off").await?;
    let released = prompt(&fixture).await;
    fixture.client.shutdown().await;
    match refused {
        Err(ExternalProviderRuntimeError::SettingFailed { setting, value, .. }) => {
            assert_eq!(setting, ProviderSettingKind::Fast);
            assert_eq!(value, "off");
        }
        other => return Err(format!("expected a refused fast off, got {other:?}").into()),
    }
    allowed?;
    released?;
    assert_eq!(prompts_seen(&fixture), 2);
    Ok(())
}

/// Oracle: D1 V14 and the Lead's clarification — an explicit fast off whose
/// valid answer drops the fast option is a refusal: the catalog becomes the
/// one the agent returned, and the next prompt proceeds.
#[tokio::test]
async fn explicit_fast_off_reply_without_fast_option_keeps_catalog_and_input() -> TestResult {
    let fixture = AcpWireFixture::start_reactive(&["fast-option-vanishes"]).await;
    create(&fixture, settings(FastRequest::Leave)).await?;
    let refused = set(&fixture, ProviderSettingKind::Fast, "off").await;
    let observed = catalog_fast(&fixture);
    let allowed = prompt(&fixture).await;
    fixture.client.shutdown().await;
    assert!(
        matches!(
            refused,
            Err(ExternalProviderRuntimeError::SettingFailed {
                setting: ProviderSettingKind::Fast,
                ..
            })
        ),
        "{refused:?}"
    );
    assert_eq!(observed, Some(FastObserved::NoOption));
    allowed?;
    assert_eq!(prompts_seen(&fixture), 1);
    Ok(())
}
