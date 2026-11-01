//! Real workerd sockets and attachments around the core session composition.
#![forbid(unsafe_code)]

#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_process;

use collaboration_central_conformance::{ConformanceResult, JwksFixtureServer, owner_assertion};
use futures_util::FutureExt as _;
use serde_json::{Value, json};
use std::time::Duration;
use workerd_process::{WorkerdProcess, WorkerdSettings};

#[derive(Clone)]
struct Client {
    http: reqwest::Client,
}
#[path = "../tests/support/archive_report_scenarios.rs"]
mod archive_report_scenarios;
#[path = "session_scope_workerd/case_results.rs"]
mod case_results;
#[path = "../tests/support/copy_drive_overlap.rs"]
mod copy_drive_overlap;
#[path = "../tests/support/copy_owner_scenarios.rs"]
mod copy_owner_scenarios;
#[path = "../tests/support/copy_read_scenarios.rs"]
mod copy_read_scenarios;
#[path = "../tests/support/copy_wait_scenarios.rs"]
mod copy_wait_scenarios;
#[path = "../tests/support/creation_scenarios.rs"]
mod creation_scenarios;
#[path = "../tests/support/delivery_config_scenarios.rs"]
mod delivery_config_scenarios;
#[path = "../tests/support/fleet_notice_scenarios.rs"]
mod fleet_notice_scenarios;
#[path = "session_scope_workerd/forward_reopen.rs"]
mod forward_reopen;
#[path = "../tests/support/forwarded_post_deadline_boundaries.rs"]
mod forwarded_post_deadline_boundaries;
#[path = "../tests/support/forwarded_post_lifecycle_oracles.rs"]
mod forwarded_post_lifecycle_oracles;
#[path = "../tests/support/forwarded_post_oracles.rs"]
mod forwarded_post_oracles;
#[path = "../tests/support/forwarded_post_route_overlap.rs"]
mod forwarded_post_route_overlap;
#[path = "../tests/support/forwarded_post_scenarios.rs"]
mod forwarded_post_scenarios;
#[path = "../tests/support/forwarded_post_state_cases.rs"]
mod forwarded_post_state_cases;
#[path = "../tests/support/forwarded_post_state_oracles.rs"]
mod forwarded_post_state_oracles;
#[path = "../tests/support/forwarded_post_terminal_slots.rs"]
mod forwarded_post_terminal_slots;
#[path = "../tests/support/forwarded_post_wire.rs"]
mod forwarded_post_wire;
#[path = "../tests/support/holder_full_wait.rs"]
mod holder_full_wait;
#[path = "../tests/support/holder_mapping_scenarios.rs"]
mod holder_mapping_scenarios;
#[path = "../tests/support/holder_writer_retry.rs"]
mod holder_writer_retry;
#[path = "../tests/support/http_body_scenarios.rs"]
mod http_body_scenarios;
#[path = "../tests/support/imported_claim_scenarios.rs"]
mod imported_claim_scenarios;
#[path = "../tests/support/mail_submit_scenarios.rs"]
mod mail_submit_scenarios;
#[path = "../tests/support/mailbox_cancel_scenarios.rs"]
mod mailbox_cancel_scenarios;
#[path = "../tests/support/mailbox_capacity_scenarios.rs"]
mod mailbox_capacity_scenarios;
#[path = "../tests/support/mailbox_deadline_scenarios.rs"]
mod mailbox_deadline_scenarios;
#[path = "../tests/support/mailbox_held_retention_scenario.rs"]
mod mailbox_held_retention_scenario;
#[path = "../tests/support/mailbox_identity_scenarios.rs"]
mod mailbox_identity_scenarios;
#[path = "../tests/support/mailbox_report_terminal_scenario.rs"]
mod mailbox_report_terminal_scenario;
#[path = "../tests/support/mailbox_rule_fixture.rs"]
mod mailbox_rule_fixture;
#[path = "../tests/support/malformed_frame_scenarios.rs"]
mod malformed_frame_scenarios;
#[path = "session_scope_workerd/normal_capability_cases.rs"]
mod normal_capability_cases;
#[path = "session_scope_workerd/normal_holder_cases.rs"]
mod normal_holder_cases;
#[path = "session_scope_workerd/normal_session_cases.rs"]
mod normal_session_cases;
#[path = "session_scope_workerd/normal_session_support.rs"]
mod normal_session_support;
#[path = "../tests/support/owner_registry_scenarios.rs"]
mod owner_registry_scenarios;
#[path = "../tests/support/mail_relay_transport.rs"]
mod relay_transport;
#[path = "../tests/support/retirement_claim_scenarios.rs"]
mod retirement_claim_scenarios;
#[path = "../tests/support/sender_control_scenarios.rs"]
mod sender_control_scenarios;
#[path = "session_scope_workerd/template_config_refusals.rs"]
mod template_config_refusals;

#[path = "../tests/support/installation_peer_directory_scenarios.rs"]
mod installation_peer_directory_scenarios;

enum Scenario {
    MailboxReportTerminal,
    MailboxHeldRetention,
    MailboxIdentity,
    MailboxCapacity,
    MailboxCancel,
    MailboxDeadline,
    DeliveryConfigRefusals,
    ForwardReopen,
    ForwardPost(forwarded_post_scenarios::ForwardCase),
    ClaimRetiring,
    ClaimRetired,
    ClaimArchived,
    ImportedClaim,
    ArchiveReports,
    Enrollment,
    InstallationDirectory,
    Admission,
    Renewal,
    Holder,
    HolderWriter,
    HolderFullWait,
    CopyWait,
    CopyOwner,
    OwnerRegistry,
    OwnerRetirement,
    CopyDriveOverlap,
    AccessCarryover,
    SessionCarryover,
    SessionCapabilities,
    SessionFailures,
    HolderLifecycle,
    HolderSchemaOverlap,
    HolderMapping,
    CopyReads,
    MalformedFrames,
    AccessSessions,
    RevocationRace,
    ReadmissionActivation,
    ReadmissionOpen,
    MailStatus,
    MailSubmitAdmission,
    RawSubmitRefusal,
    SenderSettlement,
    SenderRetirement,
    HttpBodyKeepalive,
    HttpBodyOverLimit,
    CreationGates,
    FleetNotices,
    DeliveryConfig {
        relay_peer: fleet_sync_protocol::PeerId,
    },
}
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut args = std::env::args().skip(1);
    let node = args.next().ok_or("Node executable")?.into();
    let miniflare = args.next().ok_or("pinned Miniflare entrypoint")?.into();
    let persistence = args.next().ok_or("fresh persistence")?.into();
    let port = args.next().ok_or("port")?.parse::<u16>()?;
    let scenario = match args.next().as_deref() {
        None => return Err("an explicit normal-session scenario is required".into()),
        Some("forward-reopen") => Scenario::ForwardReopen,
        Some("forward-origin") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::OriginAndReply)
        }
        Some("forward-disconnected") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::Disconnected)
        }
        Some("forward-reconnect") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::Reconnected)
        }
        Some("forward-capacity") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::Capacity)
        }
        Some("forward-deadline") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::Deadline)
        }
        Some("forward-boundary") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::ExactBoundary)
        }
        Some("forward-overlap") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::RouteOverlap)
        }
        Some("forward-state") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::OwnState)
        }
        Some("forward-terminal") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::TerminalRelease)
        }
        Some("forward-real-timer") => {
            Scenario::ForwardPost(forwarded_post_scenarios::ForwardCase::RealTimer)
        }
        Some("claim-retiring") => Scenario::ClaimRetiring,
        Some("claim-retired") => Scenario::ClaimRetired,
        Some("claim-archived") => Scenario::ClaimArchived,
        Some("imported-claim") => Scenario::ImportedClaim,
        Some("archive-reports") => Scenario::ArchiveReports,
        Some("enrollment") => Scenario::Enrollment,
        Some("installation-directory") => Scenario::InstallationDirectory,
        Some("admission") => Scenario::Admission,
        Some("renewal") => Scenario::Renewal,
        Some("holder") => Scenario::Holder,
        Some("holder-writer") => Scenario::HolderWriter,
        Some("holder-full-wait") => Scenario::HolderFullWait,
        Some("copy-wait") => Scenario::CopyWait,
        Some("copy-owner") => Scenario::CopyOwner,
        Some("owner-registry") => Scenario::OwnerRegistry,
        Some("owner-retirement") => Scenario::OwnerRetirement,
        Some("copy-drive-overlap") => Scenario::CopyDriveOverlap,
        Some("access-carryover") => Scenario::AccessCarryover,
        Some("session-carryover") => Scenario::SessionCarryover,
        Some("session-capabilities") => Scenario::SessionCapabilities,
        Some("session-failures") => Scenario::SessionFailures,
        Some("holder-lifecycle") => Scenario::HolderLifecycle,
        Some("holder-schema-overlap") => Scenario::HolderSchemaOverlap,
        Some("holder-mapping") => Scenario::HolderMapping,
        Some("copy-reads") => Scenario::CopyReads,
        Some("malformed-frames") => Scenario::MalformedFrames,
        Some("access-sessions") => Scenario::AccessSessions,
        Some("revocation-race") => Scenario::RevocationRace,
        Some("readmission-activation") => Scenario::ReadmissionActivation,
        Some("readmission-open") => Scenario::ReadmissionOpen,
        Some("mail-status") => Scenario::MailStatus,
        Some("mailbox-deadline") => Scenario::MailboxDeadline,
        Some("mailbox-report-terminal") => Scenario::MailboxReportTerminal,
        Some("mailbox-held-retention") => Scenario::MailboxHeldRetention,
        Some("mailbox-identity") => Scenario::MailboxIdentity,
        Some("mailbox-capacity") => Scenario::MailboxCapacity,
        Some("mailbox-cancel") => Scenario::MailboxCancel,
        Some("mail-submit-admission") => Scenario::MailSubmitAdmission,
        Some("raw-submit-refusal") => Scenario::RawSubmitRefusal,
        Some("sender-settlement") => Scenario::SenderSettlement,
        Some("sender-retirement") => Scenario::SenderRetirement,
        Some("http-body-keepalive") => Scenario::HttpBodyKeepalive,
        Some("http-body-over-limit") => Scenario::HttpBodyOverLimit,
        Some("creation-gates") => Scenario::CreationGates,
        Some("fleet-notices") => Scenario::FleetNotices,
        Some("delivery-config") => Scenario::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                args.next().ok_or("explicit configured relay peer")?,
            )?,
        },
        Some("delivery-config-refusals") => Scenario::DeliveryConfigRefusals,
        Some(_) => return Err("unknown scenario".into()),
    };
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let jwks = if matches!(scenario, Scenario::AccessCarryover) {
        JwksFixtureServer::start_with_cache_control(
            collaboration_central_conformance::fixture_key_profile_jwks()?,
            "public, max-age=0",
        )
        .await?
    } else {
        JwksFixtureServer::start().await?
    };
    let settings = WorkerdSettings {
        relay_peer: match &scenario {
            Scenario::DeliveryConfig { relay_peer } => *relay_peer,
            _ => fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        node,
        miniflare,
        persistence,
        port,
        jwks_url: jwks.url.clone(),
        holder_answer_timeout: std::time::Duration::from_secs(
            if matches!(
                scenario,
                Scenario::HolderFullWait
                    | Scenario::CopyDriveOverlap
                    | Scenario::SessionCapabilities
            ) {
                20
            } else {
                2
            },
        ),
        copy_ack_wait: std::time::Duration::from_secs(
            if matches!(scenario, Scenario::CopyDriveOverlap) {
                20
            } else {
                2
            },
        ),
    };
    if matches!(scenario, Scenario::ForwardReopen) {
        let result = forward_reopen::run(&settings).await;
        let keys_stopped = jwks.shutdown().await;
        result?;
        keys_stopped?;
        scenario.print_success();
        return Ok(());
    }
    if matches!(scenario, Scenario::DeliveryConfigRefusals) {
        let result = template_config_refusals::run(&settings).await;
        let stopped = jwks.shutdown().await;
        result?;
        stopped?;
        scenario.print_success();
        return Ok(());
    }
    let client = Client {
        http: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()?,
    };
    let server = match WorkerdProcess::start(&settings).await {
        Ok(server) => server,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let result = std::panic::AssertUnwindSafe(async { match scenario {
        Scenario::MailboxReportTerminal => mailbox_report_terminal_scenario::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MailboxHeldRetention => mailbox_held_retention_scenario::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MailboxDeadline => mailbox_deadline_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MailboxIdentity => mailbox_identity_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MailboxCapacity => mailbox_capacity_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MailboxCancel => mailbox_cancel_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::DeliveryConfigRefusals => Err("config refusal scenario owns its process lifetimes".into()),
        Scenario::ForwardReopen => Err("reopen scenario must own its two process lifetimes".into()),
        Scenario::ForwardPost(case) => forwarded_post_scenarios::run(&format!("http://127.0.0.1:{port}"), case, normal_session_support::reconnect_forwarding_peer).await,
        Scenario::ClaimRetiring => retirement_claim_scenarios::run(&format!("http://127.0.0.1:{port}"),retirement_claim_scenarios::ExpectedRetirement::Retiring).await,
        Scenario::ClaimRetired => retirement_claim_scenarios::run(&format!("http://127.0.0.1:{port}"),retirement_claim_scenarios::ExpectedRetirement::Retired).await,
        Scenario::ClaimArchived => imported_claim_scenarios::run_archived(&format!("http://127.0.0.1:{port}")).await,
        Scenario::ImportedClaim => imported_claim_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::ArchiveReports => archive_report_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::OwnerRetirement => owner_registry_scenarios::retire_imported_fixture(&format!("http://127.0.0.1:{port}")).await,
        Scenario::OwnerRegistry => owner_registry_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::FleetNotices => fleet_notice_scenarios::feed_advancement_reaches_reader_once(&format!("http://127.0.0.1:{port}")).await,
        Scenario::InstallationDirectory => installation_peer_directory_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::DeliveryConfig { relay_peer } => delivery_config_scenarios::run(&format!("http://127.0.0.1:{port}"),relay_peer).await,
        Scenario::CreationGates => creation_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::HttpBodyKeepalive => http_body_scenarios::refused_then_valid_enrollment_uses_the_same_physical_connection(&format!("http://127.0.0.1:{port}")).await,
        Scenario::HttpBodyOverLimit => http_body_scenarios::an_over_limit_body_is_refused_and_the_connection_is_closed(&format!("http://127.0.0.1:{port}")).await,
        Scenario::RawSubmitRefusal => sender_control_scenarios::raw_submit_is_refused_without_custody_and_the_same_socket_remains_usable(&format!("http://127.0.0.1:{port}")).await,
        Scenario::SenderSettlement => sender_control_scenarios::sender_query_and_settlement_preserve_existing_custody_and_pin_unknown_identity(&format!("http://127.0.0.1:{port}")).await,
        Scenario::SenderRetirement => sender_control_scenarios::retirement_uses_the_verified_sender_and_replays_its_original_call_receipt(&format!("http://127.0.0.1:{port}")).await,
        Scenario::CopyReads => copy_read_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::MalformedFrames => {
            malformed_frame_scenarios::run(&format!("http://127.0.0.1:{port}")).await
        }
        Scenario::HolderMapping => {
            holder_mapping_scenarios::run(&format!("http://127.0.0.1:{port}")).await
        }
        Scenario::SessionCarryover => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_session_replacement_conformance(&shared).await
        }
        Scenario::HolderSchemaOverlap => normal_holder_cases::beside_schema(&format!("http://127.0.0.1:{port}")).await,
        Scenario::HolderLifecycle => normal_holder_cases::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::SessionFailures => normal_session_cases::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::SessionCapabilities => normal_capability_cases::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::AccessCarryover => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            async {
                collaboration_central_conformance::run_access_zero_age_conformance(
                    &shared,
                    &jwks.requests,
                )
                .await?;
                collaboration_central_conformance::run_access_key_profile_conformance(&shared)
                    .await?;
                collaboration_central_conformance::run_access_verifier_control_conformance(&shared)
                    .await?;
                collaboration_central_conformance::run_access_expiry_conformance(&shared).await
            }
            .await
        }
        Scenario::AccessSessions => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_shared_access_conformance(
                &shared,
                &jwks.requests,
            )
            .await
        }
        Scenario::RevocationRace => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_revocation_race_conformance(&shared).await
        }
        Scenario::ReadmissionActivation => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_readmission_activation_conformance(&shared).await
        }
        Scenario::ReadmissionOpen => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_readmission_open_conformance(&shared).await
        }
        Scenario::MailStatus => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_empty_status_stream_conformance(&shared).await
        }
        Scenario::MailSubmitAdmission => {
            mail_submit_scenarios::run_ungranted_mail_refusal(&format!("http://127.0.0.1:{port}"))
                .await
        }
        Scenario::Enrollment => {
            let client = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_enrollment_identity_conformance(&client).await
        }
        Scenario::CopyOwner => copy_owner_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::CopyWait => copy_wait_scenarios::run(&format!("http://127.0.0.1:{port}")).await,
        Scenario::CopyDriveOverlap => {
            let base = format!("http://127.0.0.1:{port}");
            copy_drive_overlap::run(&base, || async {
                Ok(client.http.post(format!("{base}/__conformance/admission"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"kind":"holderFrames","value":{"service":copy_drive_overlap::SERVICE}}))
                    .send().await?.error_for_status()?.json::<Vec<fleet_sync_protocol::FrameId>>().await?)
            }).await
        }
        Scenario::HolderWriter => {
            let directory = tempfile::tempdir()?;
            holder_writer_retry::run(
                &format!("http://127.0.0.1:{port}"),
                &directory.path().join("writer.db"),
            )
            .await
        }
        Scenario::HolderFullWait => {
            let base = format!("http://127.0.0.1:{port}");
            holder_full_wait::run(&base, || async {
                Ok(client.http.post(format!("{base}/__conformance/admission"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"kind":"holderFrames","value":{"service":holder_full_wait::SERVICE}}))
                    .send().await?.error_for_status()?.json::<Vec<fleet_sync_protocol::FrameId>>().await?)
            }, || async {
                Ok(client.http.post(format!("{base}/__conformance/admission"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"kind":"holderDeadline","value":{"service":holder_full_wait::SERVICE}}))
                    .send().await?.error_for_status()?.json::<Option<collaboration_central_core::UnixMillis>>().await?)
            }).await
        }
        Scenario::Holder => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_holder_reply_conformance(&shared).await
        }
        Scenario::Renewal => {
            let shared = collaboration_central_conformance::ConformanceClient::new(&format!(
                "http://127.0.0.1:{port}"
            ))?;
            collaboration_central_conformance::run_lease_renewal_conformance(&shared).await
        }
        Scenario::Admission => {
            let base = format!("http://127.0.0.1:{port}");
            let shared = collaboration_central_conformance::ConformanceClient::new(&base)?;
            collaboration_central_conformance::run_admitted_session_conformance(&shared, |peer| {
                let base = &base;
                let http = &client.http;
                async move {
                let origins: Vec<collaboration_central_core::AdmissionOrigin> = http
                    .post(format!("{base}/__conformance/admission"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"kind":"origins","value":{"service":"550e8400-e29b-41d4-a716-446655440001"}}))
                    .send().await?.error_for_status()?.json().await?;
                if origins.len() != 1 || origins.first().is_none_or(|origin| origin.installation.as_str() != "550e8400-e29b-41d4-a716-446655440001" || origin.machine.as_str() != "original admission label") {
                    return Err("real DO attachment did not retain the admitted origin after relabel".into());
                }
                let report: Value = http.post(format!("{base}/__conformance/admission"))
                    .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
                    .json(&json!({"kind":"schemaReport","value":{"peer":peer}}))
                    .send().await?.error_for_status()?.json().await?;
                if report != json!({"peer":peer,"sessionId":"019f0000-0000-7000-8000-000000000450","attachSeq":1,"support":{"maxVersion":1}}) {
                    return Err("Welcome did not follow the exact committed DO Registry schema report".into());
                }
                Ok(())
            }}).await
        }
    }}).catch_unwind().await.unwrap_or_else(|_| Err("workerd scenario assertion failed".into()));
    let stopped = server.stop().await;
    let fixture = jwks.shutdown().await;
    result?;
    stopped?;
    fixture?;
    scenario.print_success();
    Ok(())
}
