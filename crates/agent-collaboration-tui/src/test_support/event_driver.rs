//! Drives the real iocraft render loop; acknowledgements distinguish ignored input
//! from a missing redraw. No sleep, polling timer, or redraw timeout is an oracle.
use super::frame_grid::FrameGrid;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;
use futures_util::{FutureExt, Stream, StreamExt};
use iocraft::{Canvas, ComponentDrawer, Hook, Hooks, MockTerminalConfig, TerminalEvent};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceSnapshot(pub Vec<(SurfaceId, Vec<u8>)>);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventAck {
    pub sequence: usize,
    pub drawn_frames: usize,
    pub snapshot: SurfaceSnapshot,
    pub handlers: Vec<String>,
}

#[derive(Clone)]
pub struct EventJournal {
    frames: Arc<AtomicUsize>,
    events: Arc<AtomicUsize>,
    handlers: Arc<Mutex<Vec<String>>>,
    acknowledgements: mpsc::UnboundedSender<EventAck>,
}
impl EventJournal {
    pub fn log_handler(&self, name: impl Into<String>) {
        self.handlers.lock().unwrap().push(name.into());
    }
    pub fn processed(&self, snapshot: SurfaceSnapshot) {
        self.acknowledgements
            .send(EventAck {
                sequence: self.events.fetch_add(1, Ordering::SeqCst) + 1,
                drawn_frames: self.draw_count(),
                snapshot,
                handlers: self.handlers.lock().unwrap().clone(),
            })
            .unwrap();
    }
    pub fn draw_count(&self) -> usize {
        self.frames.load(Ordering::SeqCst)
    }
}
struct DrawJournal(EventJournal);
impl Hook for DrawJournal {
    fn pre_component_draw(&mut self, _drawer: &mut ComponentDrawer) {
        self.0.frames.fetch_add(1, Ordering::SeqCst);
    }
}
pub fn record_draw(hooks: &mut Hooks<'_, '_>, journal: &EventJournal) {
    hooks.use_hook(|| DrawJournal(journal.clone())).0 = journal.clone();
}

#[derive(Clone, Copy, Debug)]
pub enum InputMode {
    Burst,
    FrameSeparated,
}
pub struct EventDriver {
    sender: mpsc::UnboundedSender<TerminalEvent>,
    acknowledgements: mpsc::UnboundedReceiver<EventAck>,
    pub frames: Vec<FrameGrid>,
}
impl EventDriver {
    pub fn new() -> (Self, MockTerminalConfig, EventJournal) {
        super::require_color_environment();
        let (sender, receiver) = mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let (acknowledgements, receiver) = mpsc::unbounded_channel();
        let journal = EventJournal {
            frames: Arc::new(AtomicUsize::new(0)),
            events: Arc::new(AtomicUsize::new(0)),
            handlers: Arc::new(Mutex::new(Vec::new())),
            acknowledgements,
        };
        (
            Self {
                sender,
                acknowledgements: receiver,
                frames: Vec::new(),
            },
            MockTerminalConfig::with_events(events),
            journal,
        )
    }
    pub async fn initial(&mut self, canvases: &mut (impl Stream<Item = Canvas> + Unpin)) {
        let canvas = canvases.next().await.expect("initial frame");
        self.frames
            .push(FrameGrid::from_canvas(&canvas).expect("known serializer grammar"));
        self.drain_ready(canvases);
    }
    pub async fn drive(
        &mut self,
        canvases: &mut (impl Stream<Item = Canvas> + Unpin),
        mode: InputMode,
        steps: Vec<(TerminalEvent, bool)>,
        burst_redraw: bool,
    ) -> Vec<EventAck> {
        self.drive_checked(canvases, mode, steps, burst_redraw, |_| {})
            .await
    }
    pub async fn drive_checked(
        &mut self,
        canvases: &mut (impl Stream<Item = Canvas> + Unpin),
        mode: InputMode,
        steps: Vec<(TerminalEvent, bool)>,
        burst_redraw: bool,
        mut verify_processed: impl FnMut(&EventAck),
    ) -> Vec<EventAck> {
        match mode {
            InputMode::Burst => {
                let count = steps.len();
                let before = self.frames.len();
                // No await, render polling, or controller handoff between sends.
                for (event, _) in steps {
                    self.sender.send(event).unwrap();
                }
                let mut acknowledgements = Vec::new();
                for _ in 0..count {
                    let acknowledgement = self.wait_ack(canvases).await;
                    verify_processed(&acknowledgement);
                    acknowledgements.push(acknowledgement);
                }
                if burst_redraw && self.frames.len() == before {
                    self.next_frame(canvases).await;
                }
                self.drain_ready(canvases);
                acknowledgements
            }
            InputMode::FrameSeparated => {
                self.separated(canvases, steps, &mut verify_processed).await
            }
        }
    }
    async fn separated(
        &mut self,
        canvases: &mut (impl Stream<Item = Canvas> + Unpin),
        steps: Vec<(TerminalEvent, bool)>,
        verify_processed: &mut impl FnMut(&EventAck),
    ) -> Vec<EventAck> {
        let mut acknowledgements = Vec::new();
        for (event, redraw) in steps {
            let before = self.frames.len();
            self.sender.send(event).unwrap();
            let acknowledgement = self.wait_ack(canvases).await;
            verify_processed(&acknowledgement);
            acknowledgements.push(acknowledgement);
            if redraw && self.frames.len() == before {
                self.next_frame(canvases).await;
            }
            self.drain_ready(canvases);
        }
        acknowledgements
    }
    async fn wait_ack(&mut self, canvases: &mut (impl Stream<Item = Canvas> + Unpin)) -> EventAck {
        loop {
            tokio::select! {
                acknowledgement = self.acknowledgements.recv() => return acknowledgement.expect("event owner still running"),
                canvas = canvases.next() => {
                    let canvas = canvas.expect("render loop ended before processing input");
                    self.frames.push(FrameGrid::from_canvas(&canvas).expect("known serializer grammar"));
                }
            }
        }
    }
    async fn next_frame(&mut self, canvases: &mut (impl Stream<Item = Canvas> + Unpin)) {
        let canvas = canvases.next().await.expect("changed frame");
        self.frames
            .push(FrameGrid::from_canvas(&canvas).expect("known serializer grammar"));
    }
    fn drain_ready(&mut self, canvases: &mut (impl Stream<Item = Canvas> + Unpin)) {
        while let Some(Some(canvas)) = canvases.next().now_or_never() {
            self.frames
                .push(FrameGrid::from_canvas(&canvas).expect("known serializer grammar"));
        }
    }
}
