//! Product tests consume the latest painted frame before choosing pointer coordinates.
use futures_util::{FutureExt, Stream, StreamExt};
use iocraft::Canvas;
use std::{io, pin::Pin};

pub(super) async fn next_current_frame<TRenderedFrames>(
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
