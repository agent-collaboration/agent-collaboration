//! Test-only mirror of the pin's MockTerminal through the public loop builder.
use futures_util::{
    Stream, StreamExt,
    stream::{self, BoxStream},
};
use iocraft::{
    Canvas, ElementExt, MockTerminalConfig, TerminalEvent,
    prelude::{Passthrough, TerminalBackend},
};
use std::{
    future::Future,
    io, mem,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::mpsc;

struct CanvasRecordingBackend {
    events: BoxStream<'static, TerminalEvent>,
    output: mpsc::UnboundedSender<Canvas>,
}
impl TerminalBackend for CanvasRecordingBackend {
    fn clear_canvas(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_canvas(&mut self, _previous: Option<&Canvas>, canvas: &Canvas) -> io::Result<()> {
        self.output
            .send(canvas.clone())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame observer closed"))
    }
    fn print_above(&mut self, _messages: &[Passthrough<'_>]) -> io::Result<()> {
        Ok(())
    }
    fn event_stream(&mut self) -> io::Result<BoxStream<'static, io::Result<TerminalEvent>>> {
        let events = mem::replace(&mut self.events, stream::pending().boxed());
        Ok(events.map(Ok).chain(stream::pending()).boxed())
    }
}
pub struct RecordingRenderLoop<'a> {
    output: mpsc::UnboundedReceiver<Canvas>,
    render_loop: Pin<Box<dyn Future<Output = io::Result<()>> + 'a>>,
    complete: bool,
}
impl Stream for RecordingRenderLoop<'_> {
    type Item = Canvas;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.complete
            && let Poll::Ready(result) = self.render_loop.as_mut().poll(context)
        {
            assert!(result.is_ok(), "recording render loop failed: {result:?}");
            self.complete = true;
        }
        self.output.poll_recv(context)
    }
}
pub fn recording_terminal_render_loop<'a, TElement: ElementExt + Send + 'a>(
    ui: &'a mut TElement,
    config: MockTerminalConfig,
) -> RecordingRenderLoop<'a> {
    let (sender, output) = mpsc::unbounded_channel();
    let backend = CanvasRecordingBackend {
        events: config.events,
        output: sender,
    };
    let render_loop = Box::pin(ui.render_loop().backend(backend).ignore_ctrl_c());
    RecordingRenderLoop {
        output,
        render_loop,
        complete: false,
    }
}
