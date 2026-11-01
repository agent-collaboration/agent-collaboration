//! Fixed debug-fixture metadata; the nine source-qualified identities stay intact.
use super::{PickerFixtureScenario, SessionsPickerRequest};
use chrono::{DateTime, FixedOffset, Utc};
use std::{io, sync::Arc};

pub type FixtureGroupingClock = Arc<dyn Fn() -> DateTime<FixedOffset> + Send + Sync>;

pub fn grouping_clock(
    scenario: Option<PickerFixtureScenario>,
) -> io::Result<Option<FixtureGroupingClock>> {
    if scenario != Some(PickerFixtureScenario::R19Grouping) {
        return Ok(None);
    }
    let now: DateTime<FixedOffset> = "2026-10-31T00:10:00-04:00"
        .parse()
        .map_err(io::Error::other)?;
    Ok(Some(Arc::new(move || now)))
}

enum FixtureRecency {
    At(&'static str),
    Missing,
    Invalid,
}
impl FixtureRecency {
    fn timestamp(self) -> io::Result<Option<i64>> {
        match self {
            Self::At(value) => Ok(Some(
                value
                    .parse::<DateTime<Utc>>()
                    .map_err(io::Error::other)?
                    .timestamp_millis(),
            )),
            Self::Missing => Ok(None),
            Self::Invalid => Ok(Some(i64::MAX)),
        }
    }
}

pub fn apply(request: SessionsPickerRequest) -> io::Result<SessionsPickerRequest> {
    let mut request = super::super::provider_filter_storyboard_fixture::apply_storyboard_variant(
        request,
        Some("source-qualified-outcome"),
    )?;
    // Keep the base's exact identity order. Change only grouping metadata.
    let metadata = [
        (
            None,
            Some("/repo/project-a"),
            FixtureRecency::At("2026-10-31T04:00:00Z"),
        ),
        (
            Some("✉️ Fixture @Fixture → you"),
            Some("/repo/project-b"),
            FixtureRecency::At("2026-10-31T03:59:59Z"),
        ),
        (
            Some("📋 Router: board notification @Fixture"),
            Some("/repo/project-a"),
            FixtureRecency::At("2026-10-26T04:00:00Z"),
        ),
        (
            Some("⏰ Router wake @Fixture"),
            Some("/repo/project-a"),
            FixtureRecency::At("2026-10-26T03:59:59Z"),
        ),
        (
            Some("❓ Router question @Fixture · Fixture asks"),
            Some("/repo/project-a"),
            FixtureRecency::Missing,
        ),
        (
            Some("🧵 Router: new thread activity @Fixture · 1 thread · 2 messages"),
            None,
            FixtureRecency::Invalid,
        ),
        (
            Some("✉️ Fixture @Fixture → you"),
            Some("/fixture/a"),
            FixtureRecency::At("2026-10-31T04:01:00Z"),
        ),
        (
            Some("📋 Router: board notification @Fixture"),
            Some("/fixture/b"),
            FixtureRecency::At("2026-10-30T16:00:00Z"),
        ),
        (
            Some("✉️ Fixture @Fixture → you"),
            Some("/fixture/a"),
            FixtureRecency::At("2026-10-27T16:00:00Z"),
        ),
    ];
    if request.records.len() != metadata.len() {
        return Err(io::Error::other(
            "R19 grouping fixture requires nine base identities",
        ));
    }
    for (record, (header, folder, recency)) in request.records.iter_mut().zip(metadata) {
        record.first_user_message=header.map_or_else(||"Owner-authored request".to_owned(),|header|format!("{header} · router://018f47d2-24d5-7a68-b9ec-6f759c39458f/push/018f47d2-24d5-7a68-b9ec-6f759c39458f"));
        record.cwd = folder.map(str::to_owned);
        record.normalized_cwd = record.cwd.clone();
        record.context = folder
            .and_then(|folder| folder.rsplit('/').next())
            .unwrap_or("-")
            .to_owned();
        record.recency_at_ms = recency.timestamp()?;
    }
    Ok(request)
}
