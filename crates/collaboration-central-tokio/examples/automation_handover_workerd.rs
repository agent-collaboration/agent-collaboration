//! Shared central handover proof uses real workerd owners and physical process reopen.
#![forbid(unsafe_code)]

#[cfg(debug_assertions)]
use collaboration_central_conformance::{
    AutomationClockControl, AutomationHandoverRestartCase, ConformanceClient, ConformanceResult,
    JwksFixtureServer, prepare_automation_handover_restart, resume_automation_handover_restart,
    run_automation_handover_carried_conformance, run_automation_handover_conformance,
    run_automation_handover_external_conformance, run_automation_handover_lifecycle_conformance,
    run_automation_handover_repair_conformance, run_automation_handover_validation_conformance,
    run_automation_handover_wake_conformance,
};
#[cfg(debug_assertions)]
use collaboration_central_core::RegistryTime;
#[cfg(debug_assertions)]
#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
#[expect(
    dead_code,
    reason = "This example uses configured startup; the shared process harness also serves other proof drivers."
)]
mod workerd_harness;
#[cfg(debug_assertions)]
use workerd_harness::{WorkerdProcess, WorkerdSettings};

#[cfg(debug_assertions)]
#[derive(Clone, Copy)]
enum HandoverCase {
    Receipt,
    Carried,
    External,
    Wake,
    Lifecycle,
    Validation,
    Repair,
    RestartFuture,
    RestartDueNow,
}

#[cfg(debug_assertions)]
impl HandoverCase {
    fn name(self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::Carried => "carried",
            Self::External => "external",
            Self::Wake => "wake",
            Self::Lifecycle => "lifecycle",
            Self::Validation => "validation",
            Self::Repair => "repair",
            Self::RestartFuture => "restart-future",
            Self::RestartDueNow => "restart-due-now",
        }
    }
}

#[cfg(debug_assertions)]
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut arguments = std::env::args().skip(1);
    let node = arguments.next().ok_or("Node executable")?.into();
    let miniflare = arguments
        .next()
        .ok_or("pinned Miniflare entrypoint")?
        .into();
    let persistence = std::path::PathBuf::from(arguments.next().ok_or("fresh persistence root")?);
    let port = arguments.next().ok_or("loopback port")?.parse::<u16>()?;
    let bundle = std::path::PathBuf::from(arguments.next().ok_or("debug alarm adapter bundle")?);
    let selected = arguments.next();
    if arguments.next().is_some() {
        return Err("unexpected workerd handover argument".into());
    }
    let mut cases = vec![
        HandoverCase::Receipt,
        HandoverCase::Carried,
        HandoverCase::External,
        HandoverCase::Wake,
        HandoverCase::Lifecycle,
        HandoverCase::Validation,
        HandoverCase::Repair,
        HandoverCase::RestartFuture,
        HandoverCase::RestartDueNow,
    ];
    if let Some(selected) = selected {
        cases.retain(|case| case.name() == selected);
        if cases.is_empty() {
            return Err("unknown handover case".into());
        }
    }
    if persistence.try_exists()? {
        return Err("handover workerd proof requires a fresh persistence root".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let keys = JwksFixtureServer::start().await?;
    let mut settings = WorkerdSettings {
        node,
        miniflare,
        persistence: persistence.clone(),
        port,
        jwks_url: keys.url.clone(),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()?,
        holder_answer_timeout: std::time::Duration::from_secs(2),
        copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let proof: ConformanceResult<()> = async {
        for case in cases {
            settings.persistence = persistence.join(case.name());
            run_case(case, &settings, &bundle).await?;
        }
        Ok(())
    }
    .await;
    let stopped_keys = keys.shutdown().await;
    proof?;
    stopped_keys?;
    println!(
        "AUTOMATION_HANDOVER_RESULT shared central handover cases passed; 0 failed; native effects unverified"
    );
    Ok(())
}

#[cfg(debug_assertions)]
async fn run_case(
    case: HandoverCase,
    settings: &WorkerdSettings,
    bundle: &std::path::Path,
) -> ConformanceResult<()> {
    let base = format!("http://127.0.0.1:{}", settings.port);
    let server =
        WorkerdProcess::start_configured(settings, &settings.relay_peer.to_string(), Some(bundle))
            .await?;
    let proof: ConformanceResult<Option<AutomationHandoverRestartCase>> = async {
        let client = ConformanceClient::new(&base)?;
        client
            .automation_clock(AutomationClockControl::Start {
                now: RegistryTime("2026-10-31T10:00:00Z".parse()?),
            })
            .await?;
        match case {
            HandoverCase::Receipt => run_automation_handover_conformance(&client).await?,
            HandoverCase::Carried => run_automation_handover_carried_conformance(&client).await?,
            HandoverCase::External => run_automation_handover_external_conformance(&client).await?,
            HandoverCase::Wake => run_automation_handover_wake_conformance(&client).await?,
            HandoverCase::Lifecycle => {
                run_automation_handover_lifecycle_conformance(&client).await?
            }
            HandoverCase::Validation => {
                run_automation_handover_validation_conformance(&client).await?
            }
            HandoverCase::Repair => run_automation_handover_repair_conformance(&client).await?,
            HandoverCase::RestartFuture => {
                return Ok(Some(
                    prepare_automation_handover_restart(&client, false).await?,
                ));
            }
            HandoverCase::RestartDueNow => {
                return Ok(Some(
                    prepare_automation_handover_restart(&client, true).await?,
                ));
            }
        }
        Ok(None)
    }
    .await;
    let stopped = server.stop().await;
    let prepared = proof?;
    stopped?;
    if let Some(prepared) = prepared {
        let server = WorkerdProcess::start_configured(
            settings,
            &settings.relay_peer.to_string(),
            Some(bundle),
        )
        .await?;
        let proof =
            resume_automation_handover_restart(&ConformanceClient::new(&base)?, prepared).await;
        let stopped = server.stop().await;
        proof?;
        stopped?;
    }
    println!(
        "AUTOMATION_HANDOVER_CASE_RESULT {} passed; owned process stopped and reaped",
        case.name()
    );
    Ok(())
}

#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("central handover verification uses the debug-only controlled clock");
}
