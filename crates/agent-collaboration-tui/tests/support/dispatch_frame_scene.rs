//! Native render loop with the pin's synchronized-frame writer protocol.
#[cfg(all(feature = "test-support", debug_assertions))]
use agent_collaboration_tui::input_dispatch_receipt::InputDispatchReceiptSink;
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    focus_model::{FocusModel, TargetId, TargetSpec},
    input_owner::InputOwner,
    surface_handle::use_surface_stack,
    surface_stack::SurfaceModel,
};
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
#[derive(Props)]
pub struct DispatchSceneProps {
    pub on_routed: Handler<()>,
    #[cfg(all(feature = "test-support", debug_assertions))]
    pub sink: Option<InputDispatchReceiptSink>,
}
#[component]
pub fn DispatchScene(
    props: &DispatchSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let value = hooks.use_state(|| 0usize);
    let quitting = hooks.use_state(|| false);
    if quitting.get() {
        hooks.use_context_mut::<SystemContext>().exit();
    }
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(vec![TargetSpec::new(TargetId::new("body").unwrap())]).unwrap(),
        actions: ActionRegistry::new(
            ScopeId::new("dispatch-frames").unwrap(),
            vec![
                ActionDef {
                    id: ActionId::new("change").unwrap(),
                    title: "Change".into(),
                    toolbar_label: None,
                    keys: vec!["F8".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut value = value;
                        value.set(value.get() + 1);
                    }),
                },
                ActionDef {
                    id: ActionId::new("quit").unwrap(),
                    title: "Quit".into(),
                    toolbar_label: None,
                    keys: vec!["Ctrl+C".parse().unwrap()],
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut quitting = quitting;
                        quitting.set(true);
                    }),
                },
            ],
        )
        .unwrap(),
        primary_action: None,
    });
    let on_routed = props.on_routed.clone();
    let root = element!(InputOwner(stack,width:40u16,height:8u16,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|on_routed(())) {
        Text(content:format!("Value {}",value.get()))
    });
    #[cfg(all(feature = "test-support", debug_assertions))]
    let root = {
        let mut root = root;
        root.props.dispatch_receipt = props.sink.clone();
        root
    };
    root
}
