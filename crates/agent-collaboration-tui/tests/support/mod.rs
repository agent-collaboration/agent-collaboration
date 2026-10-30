pub mod event_driver;
pub mod frame_grid;
pub mod recording_backend;

pub fn require_color_environment() {
    assert!(
        std::env::var_os("NO_COLOR").is_none(),
        "A-layer frame checks need NO_COLOR unset: the pinned iocraft memoizes it privately (color.rs:242-250). Run with `env -u NO_COLOR`."
    );
}

pub mod dialog_scene;
pub mod input_scene;
