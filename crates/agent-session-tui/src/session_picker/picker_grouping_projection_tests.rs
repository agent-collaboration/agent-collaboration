//! Literal row identities and headings pin the consumer's grouped projection.
use super::*;
use crate::session_picker::{
    SessionGroupFacet, session_group_preferences::SessionGroupPreferences,
};
use agent_collaboration_tui::{
    actions::ActionId,
    surfaces::{surface_handle::SurfaceStackHandle, surface_stack::SurfaceId},
};
use agent_session_inventory::sessions::SessionPickerIdentity;

struct ExpectedPose {
    key: Option<KeyCode>,
    identity: SessionPickerIdentity,
    title: &'static str,
    facet: &'static str,
    heading: Option<(&'static str, usize)>,
}

async fn walk_grouped_rows(
    request: SessionsPickerRequest,
    facet: SessionGroupFacet,
    poses: Vec<ExpectedPose>,
    width: usize,
    height: usize,
) {
    let mut preferences = SessionGroupPreferences::default();
    preferences.choose(facet);
    let observed = Arc::new(Mutex::new(None));
    let surface_observer = observed.clone();
    let (input_observer, mut input_events) = tokio::sync::mpsc::unbounded_channel();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let now: chrono::DateTime<chrono::FixedOffset> = "2026-10-31T00:10:00-04:00".parse().unwrap();
    let grouping_clock: super::super::picker_surface::SessionGroupingClock = Arc::new(move || now);
    let mut picker = element!(SessionsPickerComponent(
        request,width,height,group_preferences:Some(preferences),
        grouping_clock:Some(grouping_clock),
        surface_observer:move |state| {*surface_observer.lock().unwrap()=Some(state)},
        input_observer:move |_| {let _ = input_observer.send(());}
    ));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut submitted = 0;
    let mut processed = 0;
    let mut last = String::new();
    for pose in poses {
        if let Some(key) = pose.key {
            submitted += 1;
            sender
                .send(TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, key)))
                .unwrap();
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    canvas = frames.next() => {
                        let Some(canvas) = canvas else {
                            panic!("picker ended before {}",pose.title);
                        };
                        last = canvas.to_string();
                    }
                    input = input_events.recv() => {
                        input.expect("input observer remains open until Quit");
                        processed += 1;
                    }
                }
                if processed < submitted {
                    continue;
                }
                if last.contains(&format!("❯ {}", pose.title))
                    && pose.heading.is_none_or(|(heading, count)| {
                        last.lines().any(|line| {
                            line.contains(heading) && line.contains(&format!("({count})"))
                        })
                    })
                {
                    return last.clone();
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "waiting for {} with heading {:?} in {} at {width}x{height}; inputs {}/{}: {last}",
                pose.title, pose.heading, pose.facet, processed, submitted
            )
        });
        let (stack, model): (SurfaceStackHandle, State<SessionsPickerModel>) =
            observed.lock().unwrap().unwrap();
        assert_eq!(
            model.read().focused_identity(),
            Some(&pose.identity),
            "{frame}"
        );
        let snapshot = stack.snapshot();
        let registry = &snapshot.surface(SurfaceId::Screen).unwrap().actions;
        let checked = registry
            .definitions()
            .iter()
            .find(|action| action.id == ActionId::new(format!("group-{}", pose.facet)).unwrap())
            .unwrap()
            .choice
            .as_ref()
            .unwrap()
            .get();
        assert!(checked, "{frame}");
    }
    sender.send(ctrl_key('d')).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while frames.next().await.is_some() {}
    })
    .await
    .expect("grouped picker exits after Quit");
}

fn request_with_rows(records: Vec<SessionPickerRecord>) -> SessionsPickerRequest {
    let mut request = picker_request();
    request.root = SessionsPickerRoot::Any;
    request.source = SessionsSource::All;
    request.records = records;
    request
}

#[tokio::test]
async fn grouping_started_by_uses_structural_first_message_at_all_four_grids() {
    let marker = "✉️ Fixture @Fixture → you · router://018f47d2-24d5-7a68-b9ec-6f759c39458f/push/018f47d2-24d5-7a68-b9ec-6f759c39458f";
    let mut spoof = picker_record("spoof", "Title spoof", "/repo/project-a", "provider", "cli");
    spoof.full_title = marker.to_owned();
    spoof.first_user_message = "Owner's request".to_owned();
    spoof.recency_at_ms = Some(300);
    let mut informal = picker_record(
        "informal",
        "Informal note",
        "/repo/project-a",
        "provider",
        "cli",
    );
    informal.first_user_message = "✉️ notes".to_owned();
    informal.recency_at_ms = Some(200);
    let mut routed = picker_record(
        "routed",
        "Router message",
        "/repo/project-a",
        "provider",
        "cli",
    );
    routed.first_user_message = marker.to_owned();
    routed.recency_at_ms = Some(400);
    for (width, height) in [(120, 36), (76, 30), (48, 12), (32, 24)] {
        let poses = vec![
            ExpectedPose {
                key: Some(KeyCode::Home),
                identity: spoof.identity.clone(),
                title: "Title spoof",
                facet: "started-by",
                heading: Some(("You", 2)),
            },
            ExpectedPose {
                key: Some(KeyCode::Down),
                identity: informal.identity.clone(),
                title: "Informal note",
                facet: "started-by",
                heading: None,
            },
            ExpectedPose {
                key: Some(KeyCode::Down),
                identity: routed.identity.clone(),
                title: "Router message",
                facet: "started-by",
                heading: Some(("Messages", 1)),
            },
        ];
        walk_grouped_rows(
            request_with_rows(vec![spoof.clone(), informal.clone(), routed.clone()]),
            SessionGroupFacet::StartedBy,
            poses,
            width,
            height,
        )
        .await;
    }
}

#[tokio::test]
async fn grouping_day_calendar_boundaries_reach_the_list_at_all_four_grids() {
    let cases = [
        ("midnight", "Midnight", "2026-10-31T04:00:00Z", "Today"),
        (
            "previous-minute",
            "Before midnight",
            "2026-10-31T03:59:59Z",
            "Yesterday",
        ),
        ("monday", "Monday", "2026-10-26T04:00:00Z", "This week"),
        (
            "previous-week",
            "Previous week",
            "2026-10-26T03:59:59Z",
            "Older",
        ),
    ];
    let records = cases
        .iter()
        .map(|(id, title, timestamp, _)| {
            let mut record = picker_record(id, title, "/repo/project-a", "provider", "cli");
            record.recency_at_ms = Some(
                timestamp
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .unwrap()
                    .timestamp_millis(),
            );
            record
        })
        .collect::<Vec<_>>();
    for (width, height) in [(120, 36), (76, 30), (48, 12), (32, 24)] {
        let poses = cases
            .iter()
            .enumerate()
            .map(|(index, (_, title, _, heading))| ExpectedPose {
                key: Some(if index == 0 {
                    KeyCode::Home
                } else {
                    KeyCode::Down
                }),
                identity: records[index].identity.clone(),
                title,
                facet: "day",
                heading: Some((heading, 1)),
            })
            .collect();
        walk_grouped_rows(
            request_with_rows(records.clone()),
            SessionGroupFacet::Day,
            poses,
            width,
            height,
        )
        .await;
    }
}

#[tokio::test]
async fn grouping_keeps_full_identity_through_navigation_and_regrouping_at_all_four_grids() {
    let mut hosted = picker_record(
        "same",
        "Hosted choice",
        "/repo/project-a",
        "provider",
        "cli",
    );
    hosted.identity=SessionPickerIdentity::HostedCodex(serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"same"})).unwrap());
    hosted.source_context = Some(crate::session_picker::PickerSourceContext::DefaultHosted);
    hosted.recency_at_ms = Some(100);
    let mut local = picker_record("same", "Local choice", "/repo/project-a", "provider", "cli");
    local.source_context = Some(crate::session_picker::PickerSourceContext::LocalCodex);
    local.recency_at_ms = Some(200);
    for (width, height) in [(120, 36), (76, 30), (48, 12), (32, 24)] {
        let mut poses = vec![
            ExpectedPose {
                key: None,
                identity: local.identity.clone(),
                title: "Local choice",
                facet: "machine",
                heading: None,
            },
            ExpectedPose {
                key: Some(KeyCode::Home),
                identity: hosted.identity.clone(),
                title: "Hosted choice",
                facet: "machine",
                heading: Some(("This machine", 1)),
            },
            ExpectedPose {
                key: Some(KeyCode::Down),
                identity: local.identity.clone(),
                title: "Local choice",
                facet: "machine",
                heading: Some(("Local Codex", 1)),
            },
            ExpectedPose {
                key: Some(KeyCode::Up),
                identity: hosted.identity.clone(),
                title: "Hosted choice",
                facet: "machine",
                heading: None,
            },
        ];
        for facet in ["none", "started-by", "day", "folder", "provider", "machine"] {
            poses.push(ExpectedPose {
                key: Some(KeyCode::F(6)),
                identity: hosted.identity.clone(),
                title: "Hosted choice",
                facet,
                heading: None,
            });
        }
        walk_grouped_rows(
            request_with_rows(vec![hosted.clone(), local.clone()]),
            SessionGroupFacet::Machine,
            poses,
            width,
            height,
        )
        .await;
    }
}
