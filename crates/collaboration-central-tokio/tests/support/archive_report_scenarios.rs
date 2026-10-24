//! The retained archive-report oracle runs through normal Worker sessions.

use collaboration_central_conformance::{
    ClockObservation, ConformanceClient, ConformanceResult, RuntimeControl, RuntimeControlRequest,
    RuntimeFaultObservation, run_archive_report_with_clock,
};
use collaboration_central_core::ObjectAddress;
use std::time::Duration;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    run_archive_report_with_clock(&client, |now| {
        let http = http.clone();
        async move {
            let response = super::holder_writer_retry::owner_post(
                &http,
                base,
                "runtime",
                serde_json::to_value(RuntimeControlRequest {
                    address: ObjectAddress::Fleet,
                    command: RuntimeControl::SetTime { now },
                })?,
            )
            .await?;
            let observed: RuntimeFaultObservation = serde_json::from_value(response)?;
            if !matches!(observed.clock, ClockObservation::Pinned { now: actual } if actual == now)
            {
                return Err("archive report clock did not reach the owning Fleet object".into());
            }
            Ok(())
        }
    })
    .await
}
