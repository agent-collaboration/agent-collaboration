//! Native synchronized frame output shared by component and subprocess observations.
use futures_util::{
    Stream, StreamExt,
    stream::{self, BoxStream},
};
use iocraft::prelude::*;
use std::{
    future::Future,
    io::{self, Write},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::mpsc;

#[derive(Clone, Default)]
pub struct SharedFrameOutput(Arc<Mutex<Vec<u8>>>);
impl SharedFrameOutput {
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}
impl Write for SharedFrameOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("frame output poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub struct DispatchFrame {
    pub bytes: Vec<u8>,
    pub canvas: Option<Canvas>,
}
struct DispatchBackend {
    events: BoxStream<'static, TerminalEvent>,
    writer: Box<dyn Write + Send>,
    bytes: SharedFrameOutput,
    frame_start: usize,
    canvas: Option<Canvas>,
    frames: mpsc::UnboundedSender<DispatchFrame>,
}
impl TerminalBackend for DispatchBackend {
    fn begin_frame(&mut self) -> io::Result<()> {
        self.frame_start = self.bytes.bytes().len();
        self.canvas = None;
        self.writer.write_all(b"\x1b[?2026h")?;
        self.writer.flush()
    }
    fn clear_canvas(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_canvas(&mut self, _previous: Option<&Canvas>, canvas: &Canvas) -> io::Result<()> {
        self.canvas = Some(canvas.clone());
        canvas.write_ansi(&mut self.writer)
    }
    fn end_frame(&mut self) -> io::Result<()> {
        self.writer.write_all(b"\x1b[?2026l")?;
        self.writer.flush()?;
        self.frames
            .send(DispatchFrame {
                bytes: self.bytes.bytes()[self.frame_start..].to_vec(),
                canvas: self.canvas.take(),
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame observer closed"))
    }
    fn print_above(&mut self, _messages: &[Passthrough<'_>]) -> io::Result<()> {
        Ok(())
    }
    fn event_stream(&mut self) -> io::Result<BoxStream<'static, io::Result<TerminalEvent>>> {
        Ok(
            std::mem::replace(&mut self.events, stream::pending().boxed())
                .map(Ok)
                .chain(stream::pending())
                .boxed(),
        )
    }
}
pub struct DispatchRenderLoop<'a> {
    frames: mpsc::UnboundedReceiver<DispatchFrame>,
    render: Pin<Box<dyn Future<Output = io::Result<()>> + 'a>>,
    complete: bool,
}
impl Stream for DispatchRenderLoop<'_> {
    type Item = DispatchFrame;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.complete
            && let Poll::Ready(result) = self.render.as_mut().poll(context)
        {
            assert!(result.is_ok(), "native render loop failed: {result:?}");
            self.complete = true;
        }
        self.frames.poll_recv(context)
    }
}
pub fn dispatch_render_loop<'a, TElement: ElementExt + Send + 'a>(
    ui: &'a mut TElement,
    config: MockTerminalConfig,
    bytes: SharedFrameOutput,
    writer: Box<dyn Write + Send>,
) -> DispatchRenderLoop<'a> {
    let (sender, frames) = mpsc::unbounded_channel();
    let backend = DispatchBackend {
        events: config.events,
        writer,
        bytes,
        frame_start: 0,
        canvas: None,
        frames: sender,
    };
    DispatchRenderLoop {
        frames,
        render: Box::pin(ui.render_loop().backend(backend).ignore_ctrl_c()),
        complete: false,
    }
}
