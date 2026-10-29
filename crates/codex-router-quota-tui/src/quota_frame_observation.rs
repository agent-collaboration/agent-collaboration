//! Product tests consume the latest painted frame before choosing pointer coordinates.
use futures_util::{FutureExt, Stream, StreamExt};
use iocraft::Canvas;
use iocraft::prelude::ElementExt;
use std::{io, pin::Pin};

pub(crate) enum FrameJourneyStep {
    Painted {
        ready: fn(&str) -> bool,
        events: Vec<iocraft::prelude::TerminalEvent>,
    },
    Completed {
        signals: tokio::sync::mpsc::UnboundedReceiver<()>,
        count: usize,
        event: iocraft::prelude::TerminalEvent,
    },
}

/// Sends each key only after the current paint acknowledges its prerequisite.
pub(crate) async fn render_frame_journey(
    mut component: iocraft::AnyElement<'static>,
    steps: Vec<FrameJourneyStep>,
) -> Vec<String> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames =
        component.mock_terminal_render_loop(iocraft::MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut observed = Vec::new();
    for (step_index, step) in steps.into_iter().enumerate() {
        let events = match step {
            FrameJourneyStep::Painted { ready, events } => {
                let canvas = next_matching_frame(
                    frames.as_mut(),
                    std::time::Duration::from_secs(2),
                    |canvas| ready(&canvas.to_string()),
                )
                .await
                .unwrap_or_else(|error| panic!("journey step {step_index} prerequisite: {error}; prior acknowledged frame: {:?}", observed.last()));
                observed.push(canvas.to_string());
                events
            }
            FrameJourneyStep::Completed {
                mut signals,
                count,
                event,
            } => {
                assert!(count > 0, "completion step requires a producer fact");
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    let mut completed = 0;
                    while completed < count {
                        tokio::select! {
                            signal = signals.recv() => {
                                signal.expect("completion producer ended before its fact");
                                completed += 1;
                            }
                            canvas = frames.next() => {
                                observed.push(canvas.expect("render loop ended before producer completion").to_string());
                            }
                        }
                    }
                }).await.expect("actual saver or loader must complete before the next key");
                vec![event]
            }
        };
        for event in events {
            sender.send(event).expect("journey input must remain open");
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while let Some(canvas) = frames.next().await {
            observed.push(canvas.to_string());
        }
    })
    .await
    .expect("final exit event must settle the render loop");
    observed
}

/// A finite acknowledgement of caller-owned dimensions and modal or focus content.
pub(crate) async fn next_matching_frame<TRenderedFrames>(
    mut frames: Pin<&mut TRenderedFrames>,
    timeout: std::time::Duration,
    mut matches: impl FnMut(&Canvas) -> bool,
) -> io::Result<Canvas>
where
    TRenderedFrames: Stream<Item = Canvas> + ?Sized,
{
    let mut last_frame = String::new();
    let result = tokio::time::timeout(timeout, async {
        while let Some(canvas) = next_current_frame(frames.as_mut()).await? {
            if matches(&canvas) {
                return Ok(canvas);
            }
            last_frame = format!("{}x{}\n{canvas}", canvas.width(), canvas.height());
        }
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("stream ended before matching frame; last painted frame:\n{last_frame}"),
        ))
    })
    .await;
    result.map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("matching-frame deadline; last painted frame:\n{last_frame}"),
        )
    })?
}

pub(crate) async fn next_current_frame<TRenderedFrames>(
    mut frames: Pin<&mut TRenderedFrames>,
) -> io::Result<Option<Canvas>>
where
    TRenderedFrames: Stream<Item = Canvas> + ?Sized,
{
    let Some(mut current) = frames.next().await else {
        return Ok(None);
    };
    // iocraft polls rendering before returning queued output. Old queued canvases
    // can precede the current hitboxes; a repaint oscillation must fail fast.
    for _ in 0..64 {
        match frames.next().now_or_never() {
            Some(Some(painted)) => current = painted,
            _ => return Ok(Some(current)),
        }
    }
    Err(io::Error::other(format!(
        "ready-frame drain exceeded64; last painted frame:\n{current}"
    )))
}

#[tokio::test]
async fn current_frame_reader_uses_latest_queued_paint_and_preserves_completion() {
    use iocraft::prelude::*;
    let frames = vec![
        element!(Text(content:"old position")).render(None),
        element!(Text(content:"current position")).render(None),
    ];
    let mut frames = Box::pin(futures_util::stream::iter(frames));
    let current = next_current_frame(frames.as_mut()).await.unwrap().unwrap();
    assert_eq!(current.to_string(), "current position\n");
    assert!(next_current_frame(frames.as_mut()).await.unwrap().is_none());
}

#[tokio::test]
async fn current_frame_reader_rejects_excess_ready_paints_with_last_frame_evidence() {
    use iocraft::prelude::*;
    let frames = (0..65)
        .map(|index| element!(Text(content:format!("paint-{index}"))).render(None))
        .collect::<Vec<_>>();
    let mut frames = Box::pin(futures_util::stream::iter(frames));
    let error = next_current_frame(frames.as_mut())
        .await
        .err()
        .expect("excess ready paints must fail");
    assert!(error.to_string().contains("paint-64"));
}

#[tokio::test]
async fn matching_frame_waits_for_delayed_dimensions_and_content() {
    use iocraft::prelude::*;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(element!(View(width:80) {Text(content:"stale modal")}).render(None))
        .unwrap();
    let mut frames = Box::pin(futures_util::stream::unfold(
        receiver,
        |mut receiver| async { receiver.recv().await.map(|frame| (frame, receiver)) },
    ));
    let delayed = async {
        tokio::task::yield_now().await;
        sender
            .send(element!(View(width:120) {Text(content:"ready modal")}).render(None))
            .unwrap();
    };
    let (result, ()) = tokio::join!(
        next_matching_frame(
            frames.as_mut(),
            std::time::Duration::from_millis(250),
            |canvas| { canvas.width() == 120 && canvas.to_string().contains("ready modal") }
        ),
        delayed,
    );
    let canvas = result.unwrap();
    assert_eq!(canvas.width(), 120);
    assert!(canvas.to_string().contains("ready modal"));
}

#[tokio::test]
async fn matching_frame_rejects_closed_or_wrong_only_stream() {
    use iocraft::prelude::*;
    for frames in [
        Vec::new(),
        vec![element!(View(width:80) {Text(content:"wrong modal")}).render(None)],
    ] {
        let mut frames = Box::pin(futures_util::stream::iter(frames));
        let error = next_matching_frame(
            frames.as_mut(),
            std::time::Duration::from_millis(250),
            |canvas| canvas.width() == 120,
        )
        .await
        .err()
        .expect("end without requested frame must fail");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}

#[tokio::test(start_paused = true)]
async fn matching_frame_deadline_rejects_wrong_live_stream() {
    use iocraft::prelude::*;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(element!(View(width:80) {Text(content:"wrong live modal")}).render(None))
        .unwrap();
    let mut frames = Box::pin(futures_util::stream::unfold(
        receiver,
        |mut receiver| async { receiver.recv().await.map(|frame| (frame, receiver)) },
    ));
    let error = next_matching_frame(
        frames.as_mut(),
        std::time::Duration::from_millis(250),
        |canvas| canvas.width() == 120,
    )
    .await
    .err()
    .expect("wrong live stream must hit its bounded deadline");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    drop(sender);
}

#[tokio::test]
async fn matching_frame_preserves_the_ready_paint_limit() {
    use iocraft::prelude::*;
    let frames = (0..65)
        .map(|index| element!(Text(content:format!("paint-{index}"))).render(None))
        .collect::<Vec<_>>();
    let mut frames = Box::pin(futures_util::stream::iter(frames));
    let error = next_matching_frame(
        frames.as_mut(),
        std::time::Duration::from_millis(250),
        |_| true,
    )
    .await
    .err()
    .expect("matching cannot bypass ready paint bound");
    assert!(error.to_string().contains("ready-frame drain exceeded64"));
    assert!(error.to_string().contains("paint-64"));
}
