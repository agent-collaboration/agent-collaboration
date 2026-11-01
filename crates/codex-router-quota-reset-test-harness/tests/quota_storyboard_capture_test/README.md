# Real compiled terminal observation

The TypeScript entry is `scripts/verification/tui/capture/capture-run.ts` with a declared
plan; its `--check` and README explain exact artifacts and prerequisites. This
libtest's ignored `compiled_grid_terminal_observation` exposes the existing
Rust-owned PTY as a bounded read/write/reply/resize/finish/stop protocol. It does
not render a replacement UI. Actual bytes feed the pinned libghostty grid and
wterm DOM original-image sink.

`grid_terminal_capture.rs` selects sealed session/account or exact compiled
component fixtures. Reads may reach the sealed provider only; state/secrets must
remain unchanged. Component completion needs its durable scenario receipt with
zero external requests and persisted writes. Entry/read deadlines are failures,
not negative product proof. Every run keeps bytes, grids, commands and diagnostics.

`public_sessions_capture.rs` swaps only the public sink. It preserves the fixed
Node child, normal HOME/CODEX_HOME, exact public executable/cwd/Host guard and
separate effect/job-settlement contract. Public wrapper termination verifies
UID/birth/command before SIGTERM to that exact retained wrapper. Fixture immediate
termination is unchanged. Public runtime execution needs its own allowed-effects
packet and is not part of the S0 port.

The ordinary native adapter and public transport tests use controlled children,
not a real Router or provider. They exercise exact argv, environment, durable
receipts, real PTYs, kernel resize and retained-child/bystander cleanup. Use the
nominated canonical Node26.12.1 and SHA-256 for native tests. No stand-in counts
as product rendering or public-provider proof.

The canonical recipe is `.agents/skills/verify-codex-router/references/tui-journeys.md`.
