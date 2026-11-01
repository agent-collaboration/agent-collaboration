//! The controlled alarm capability still calls the generated Rust Worker handler.
#![forbid(unsafe_code)]
#[cfg(debug_assertions)]
use collaboration_central_conformance::{
    AutomationAlarmResult, AutomationClockControl, ConformanceClient, ConformanceResult,
    JwksFixtureServer,
};
#[cfg(debug_assertions)]
use collaboration_central_core::RegistryTime;
#[cfg(debug_assertions)]
#[path = "../../collaboration-central-conformance/examples/support/workerd_process.rs"]
mod workerd_harness;
#[cfg(debug_assertions)]
use workerd_harness::{WorkerdProcess, WorkerdSettings};

#[cfg(debug_assertions)]
#[tokio::main]
async fn main() -> ConformanceResult<()> {
    let mut arguments = std::env::args().skip(1);
    let node = arguments.next().ok_or("Node executable")?.into();
    let miniflare = arguments.next().ok_or("Miniflare entrypoint")?.into();
    let persistence = arguments
        .next()
        .ok_or("owned persistence directory")?
        .into();
    let port = arguments.next().ok_or("loopback port")?.parse::<u16>()?;
    let bundle = std::path::PathBuf::from(arguments.next().ok_or("debug alarm adapter bundle")?);
    if arguments.next().is_some() {
        return Err("unexpected clock argument".into());
    }
    drop(std::net::TcpListener::bind(("127.0.0.1", port))?);
    let keys = JwksFixtureServer::start().await?;
    let outcome: ConformanceResult<()> = async {
    let settings = WorkerdSettings { node, miniflare, persistence, port, jwks_url: keys.url.clone(),
        relay_peer: "019f0000-0000-7000-8000-000000005241".to_owned().try_into()?,
        holder_answer_timeout: std::time::Duration::from_secs(2), copy_ack_wait: std::time::Duration::from_secs(2),
    };
    let initial = RegistryTime("2026-10-31T10:04:59.999Z".parse()?);
    let boundary = RegistryTime("2026-10-31T10:05:00Z".parse()?);
    let client = ConformanceClient::new(&format!("http://127.0.0.1:{port}"))?;
    let server = WorkerdProcess::start(&settings).await?;
    let negative = client.automation_clock(AutomationClockControl::Start { now: initial }).await;
    let stopped = server.stop().await;
    stopped?;
    match negative {
        Err(error) if error.to_string() == "clock control omitted callback observation" => {},
        other => return Err(format!("unwrapped Worker was mistaken for a drained clock: {other:?}").into()),
    }
    let server = WorkerdProcess::start_configured(&settings, &settings.relay_peer.to_string(), Some(&bundle)).await?;
    let proof: ConformanceResult<usize> = async {
        let before = client.automation_clock(AutomationClockControl::Start { now: initial }).await?;
        let quiet = client.automation_clock(AutomationClockControl::Advance { now: boundary }).await?;
        assert_eq!(quiet.now, boundary);
        assert_eq!(quiet.callbacks.len(), before.callbacks.len(), "quiet advance manufactured callbacks");
        let injected = client.automation_clock(AutomationClockControl::InvokeAlarm).await?;
        assert_eq!(injected.callbacks.len(), quiet.callbacks.len() + 1);
        assert!(injected.callbacks.last().is_some_and(|receipt| matches!(receipt.result, AutomationAlarmResult::Completed)));
        let duplicate = client.automation_clock(AutomationClockControl::InvokeAlarm).await?;
        assert_eq!(duplicate.callbacks.len(), injected.callbacks.len() + 1);
        assert!(duplicate.next_alarm_ms.is_none());
        println!("AUTOMATION_CLOCK_BEFORE_RESTART {}", serde_json::to_string(&duplicate)?);
        Ok(duplicate.callbacks.len())
    }.await;
    let stopped = server.stop().await;
    let count = proof?;
    stopped?;
    let server = WorkerdProcess::start_configured(&settings, &settings.relay_peer.to_string(), Some(&bundle)).await?;
    let proof = client.automation_clock(AutomationClockControl::Observe).await;
    let stopped = server.stop().await;
    let reopened = proof?;
    stopped?;
    assert_eq!(reopened.now, boundary);
    assert_eq!(reopened.callbacks.len(), count);
    assert!(reopened.next_alarm_ms.is_none());
    println!("AUTOMATION_CLOCK_AFTER_RESTART {}", serde_json::to_string(&reopened)?);
    println!("AUTOMATION_CLOCK_RESULT real callbacks/quiet advance/restart passed; production scheduler timing and native retry delivery unverified");
    Ok(())
    }.await;
    let closed_keys = keys.shutdown().await;
    outcome?;
    closed_keys?;
    Ok(())
}

#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("automation verification clock is compiled only in debug builds");
}
