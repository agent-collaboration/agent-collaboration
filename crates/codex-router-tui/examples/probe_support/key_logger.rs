use iocraft::prelude::*;
use std::{
    io,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default)]
struct KeyboardJournal {
    events: Vec<String>,
    presses: usize,
    repeats: usize,
    releases: usize,
    enter_commands: usize,
    ctrl_c: usize,
}
#[derive(Props)]
struct KeyLoggerProps {
    #[iocraft(required)]
    journal: Arc<Mutex<KeyboardJournal>>,
    ctrl_c_exits: bool,
}
#[component]
fn KeyLogger(props: &KeyLoggerProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let journal = props.journal.clone();
    let ctrl_c_exits = props.ctrl_c_exits;
    let mut last_event = hooks.use_state(|| "READY".to_owned());
    let mut should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();
    // This is the only input subscriber. No separate stdin reader.
    hooks.use_terminal_events(move |event| {
        let line = format!("{event:?}");
        if let Ok(mut journal) = journal.lock() {
            journal.events.push(line.clone());
            if let TerminalEvent::Key(key) = event {
                match key.kind {
                    KeyEventKind::Press => journal.presses += 1,
                    KeyEventKind::Repeat => journal.repeats += 1,
                    KeyEventKind::Release => journal.releases += 1,
                }
                // Declared repeat policy: activate only on Press.
                if key.code == KeyCode::Enter && key.kind == KeyEventKind::Press {
                    journal.enter_commands += 1;
                }
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.kind == KeyEventKind::Press
                {
                    journal.ctrl_c += 1;
                    if ctrl_c_exits {
                        should_exit.set(true);
                    }
                }
                if key.code == KeyCode::Char('q') && key.kind == KeyEventKind::Press {
                    should_exit.set(true);
                }
            }
        }
        last_event.set(line);
    });
    if should_exit.get() {
        system.exit();
    }
    let snapshot = props
        .journal
        .lock()
        .map(|journal| journal.clone())
        .unwrap_or_default();
    element! {
        View(width: 76, height: 12, flex_direction: FlexDirection::Column) {
            Text(content: "KEY LOGGER: q exits; Enter commands only on Press")
            Text(content: format!("events={} presses={} repeats={} releases={}", snapshot.events.len(), snapshot.presses, snapshot.repeats, snapshot.releases))
            Text(content: format!("enter_commands={} ctrl_c={}", snapshot.enter_commands, snapshot.ctrl_c))
            Text(content: last_event.to_string())
        }
    }
}

pub async fn run_keyboard_probe() -> io::Result<()> {
    let ignore_ctrl_c = std::env::args().any(|arg| arg == "--ignore-ctrl-c");
    let ctrl_c_exits = std::env::args().any(|arg| arg == "--ctrl-c-exits");
    let journal = Arc::new(Mutex::new(KeyboardJournal::default()));
    let mut ui = element!(KeyLogger(journal: journal.clone(), ctrl_c_exits));
    let render_loop = ui.render_loop().fullscreen().enable_bracketed_paste();
    let result = if ignore_ctrl_c {
        render_loop.ignore_ctrl_c().await
    } else {
        render_loop.await
    };
    let snapshot = journal
        .lock()
        .map_err(|error| io::Error::other(error.to_string()))?;
    println!(
        "FINAL ignore_ctrl_c={ignore_ctrl_c} presses={} repeats={} releases={} enter_commands={} ctrl_c={}",
        snapshot.presses,
        snapshot.repeats,
        snapshot.releases,
        snapshot.enter_commands,
        snapshot.ctrl_c
    );
    for event in &snapshot.events {
        println!("EVENT {event}");
    }
    result
}
