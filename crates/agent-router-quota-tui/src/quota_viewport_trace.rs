//! Scalar fixture evidence for correlating kernel, hook, state and drawn bounds.
use iocraft::{ComponentDrawer, Hook, prelude::*};
use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
};

const MAX_VIEWPORT_SAMPLES: usize = 64;

#[derive(Clone, Default)]
pub(crate) struct QuotaViewportTrace {
    samples: Arc<Mutex<Vec<ViewportSample>>>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ViewportSample {
    pub(crate) kernel: Option<[u16; 2]>,
    pub(crate) terminal_hook: [u16; 2],
    pub(crate) observed: [usize; 2],
    pub(crate) drawn: [u16; 2],
    pub(crate) origin: [i16; 2],
}

impl QuotaViewportTrace {
    pub(crate) fn write_next_to(&self, receipt: &Path) -> io::Result<()> {
        let samples = self
            .samples
            .lock()
            .map_err(|_| io::Error::other("viewport trace lock poisoned"))?;
        if samples.is_empty() || samples.len() >= MAX_VIEWPORT_SAMPLES {
            return Err(io::Error::other(
                "viewport trace is empty or reached its sample bound",
            ));
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(receipt.with_extension("viewport-trace.json"))?;
        serde_json::to_writer_pretty(file, &*samples).map_err(io::Error::other)
    }
}

struct ViewportTraceHook {
    trace: Option<QuotaViewportTrace>,
    terminal_hook: [u16; 2],
    observed: [usize; 2],
}
impl Hook for ViewportTraceHook {
    fn post_component_draw(&mut self, drawer: &mut ComponentDrawer) {
        let Some(trace) = &self.trace else {
            return;
        };
        let position = drawer.canvas_position();
        let size = drawer.size();
        let sample = ViewportSample {
            kernel: crossterm::terminal::size()
                .ok()
                .map(|(width, height)| [width, height]),
            terminal_hook: self.terminal_hook,
            observed: self.observed,
            drawn: [size.width, size.height],
            origin: [position.x, position.y],
        };
        let mut samples = trace.samples.lock().expect("owned fixture viewport trace");
        if samples.last() != Some(&sample) && samples.len() < MAX_VIEWPORT_SAMPLES {
            samples.push(sample);
        }
    }
}

pub(crate) fn use_viewport_trace(
    hooks: &mut Hooks,
    trace: Option<QuotaViewportTrace>,
    terminal_hook: [u16; 2],
    observed: [usize; 2],
) {
    let initial_trace = trace.clone();
    let hook = hooks.use_hook(move || ViewportTraceHook {
        trace: initial_trace,
        terminal_hook,
        observed,
    });
    hook.trace = trace;
    hook.terminal_hook = terminal_hook;
    hook.observed = observed;
}

#[test]
fn viewport_trace_requires_real_samples_and_never_overwrites_retained_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let receipt = directory.path().join("completion");
    let trace = QuotaViewportTrace::default();
    assert!(trace.write_next_to(&receipt).is_err());
    trace.samples.lock().unwrap().push(ViewportSample {
        kernel: Some([120, 36]),
        terminal_hook: [160, 60],
        observed: [120, 60],
        drawn: [120, 60],
        origin: [0, 0],
    });
    trace.write_next_to(&receipt).unwrap();
    let path = receipt.with_extension("viewport-trace.json");
    let original = std::fs::read(&path).unwrap();
    assert!(trace.write_next_to(&receipt).is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(
        !receipt.exists(),
        "trace cannot manufacture the fixture completion receipt"
    );
}
