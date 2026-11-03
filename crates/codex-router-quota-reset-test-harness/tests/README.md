# Quota harness tests

- `integration/`: installation and package contracts, without launching binaries. Run `cargo nextest run -p codex-router-quota-reset-test-harness --test integration`.
- `e2e/`: Claude terminal lifecycle, quota/reset PTY journeys, and storyboard/grid observers. Run `cargo nextest run -p codex-router-quota-reset-test-harness --test e2e` only in an authorized native window. Opt-in observer bodies remain ignored.

Each kind has one Cargo target. The PTY helpers retain their colocated unit tests. The reset and storyboard families still compile separate helper instances, preserving the original 57 discovered tests, including five ignored cases; consolidating them would remove duplicate existing cases. The e2e root marks Clippy duplicate_mod as expected for this intentional composition, under the same Lead instruction; every other warning remains denied. No assertions, events or failure guards are changed by this move.

The grid observer is now `e2e` entry `quota_storyboard::compiled_grid_terminal_observation`; public Sessions uses `quota_storyboard::compiled_public_sessions_grid_observation`. Retained old executables remain historical artifacts until a fresh declaration qualifies the new target.
