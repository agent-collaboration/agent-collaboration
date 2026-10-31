# Component showcase

An ordinary public-control consumer with local keyed fixtures and recording effects.
It does not contact a Router, provider, account store or native session. Menu and
toolbar expose the demonstrations; there is no shortcut footer or second dispatcher.

Terminal size is read once at launch and passed to InputOwner. Later Resize events
use InputOwner's existing subscription; the sample installs no terminal-event or
terminal-size hooks. Sample R5 lint enforcement arrives with S8's crate
`clippy.toml`, including its `--examples` check.

Run the app from the repository root:

```sh
cargo run -p agent-collaboration-tui --example component_showcase
```

The debug `test-support` build additionally accepts exactly
`--receipt-directory ABSOLUTE_EMPTY_OWNED_DIRECTORY`. It writes dispatch ACKs and
append-only state/draw/serialized-output receipts there. Default/release builds
have no receipt route. A logical draw alone never qualifies received terminal
output. The stdout adapter forwards bytes unchanged and observes real synchronized
output endings; it does not add ANSI, markers or synthetic terminal content.
Only paired synchronized blocks count; terminal cleanup's unpaired end is excluded.
New measured views require the actual follow-up draw that uses their first width/
viewport measurement. Uncovering an existing parent does not request an arbitrary
extra frame. Callback snapshots and the draw bound to each image are retained
separately, and a newer serialized producer frame prevents accepting an older grid.
The debug journal also records the main list's public `ScrollPosition` dimensions.
When an actual draw changes those measurements, require that draw's follow-up;
an intermediate draw inside a multi-key burst can already satisfy it. Do not infer
readiness from row counts or demand an arbitrary extra frame after the final key.

For controlled completions, choose a Stage action from Menu, open the relevant
form/dropdown/Help, then use the Menu's Apply action or let the owned verifier
signal its declared fixture child. No fixture uses timed mutations. The wide-glyph
menu actions paint a real layout column of each glyph beneath all border rows.
The nested details fixture contains visible, partially clipped and hidden buttons.

## Build and drive

Use the repository's pinned toolchain and install dependencies with
`pnpm --dir scripts/verification/tui install --frozen-lockfile` and
`pnpm --dir verification install --frozen-lockfile`.

```sh
cargo build -p agent-collaboration-tui --example component_showcase --features test-support
cargo test -p codex-router-quota-reset-test-harness \
  --test quota_storyboard_capture_test --no-run --message-format=json
```

Read the unique `quota_storyboard_capture_test` executable path from Cargo's
artifact output. Sign it and the example before direct execution:

```sh
scripts/cargo_debug_signing_runner.sh --sign-only \
  target/debug/examples/component_showcase ABSOLUTE_DRIVER_PATH
node scripts/verification/tui/showcase/prepare-plan.ts \
  --driver ABSOLUTE_DRIVER_PATH \
  --example "$PWD/target/debug/examples/component_showcase" \
  --output "$PWD/tmp/component-showcase-observation" \
  --chrome '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  > tmp/component-showcase-plan.json
node scripts/verification/tui/capture-run.ts --showcase tmp/component-showcase-plan.json --check
node scripts/verification/tui/capture-run.ts --showcase tmp/component-showcase-plan.json
```

The output directory must be fresh. Pin after final edits/signing; source or
executable changes invalidate the plan. `--cases`, `--grids`, `--keys` and
`--variants` narrow a
preflight run; the default plan is all SC01–SC27, six grids and both protocols.
SC07 allocates separate children for enabled Quit from main/menu/dropdown/dialog.
SC25 allocates twelve independent scene/glyph children and SC26 three lifecycle
children, so one contrary result cannot hide a later probe. The complete matrix
has 516 cases. Two independent PTY children run concurrently with separate
receipt directories and browser profiles. SIGINT/SIGTERM stops allocating cases
and lets the active cases finish and clean up; an incomplete run returns exit 1.
Resize cases retain the same child. Use a separate fresh plan/output with
`--profile example-ghostty-keybinds` to retain consumed-key prerequisite findings.
Raw VT supplements are labelled independently from delivered profile keys.
Help checks find the actual `never quit` convention through real scrolling; at
small heights neither its section heading nor that row must be visible at End.
Receipt watches attach directly to both owned files after the initial received
frame, retaining append-only identity checks. A coalesced directory creation
notification is discovery evidence, not a separate readiness requirement.
SC19 inspects actual completed-draw ordering between the opening key and outside
press. A single PTY burst does not force both inputs to precede the first draw;
that ordering gap is a prerequisite finding, never a product failure or a retry
until a preferred outcome appears.

Each case retains exact ACKs, raw ANSI, parsed grids, state, original wterm DOM
PNGs and identity/cleanup evidence. `results.json` and `results.md` distinguish
Pass, Fail, Unobserved and blocked-by-prerequisite. Any non-pass returns exit 1;
that is not permission to repair product code or relax an assertion. Unknown
SC18 dropdown-Help restoration policy remains unobserved until the core owner
pins it. Fixed dropdown placement is exposed by SC23, not repaired by the sample.

These are build-and-drive observations. The Advisor owns the independent verdict.
Physical keyboard delivery is outside the PTY profile's claim.

The lasting journeys also cover filter-focused Enter opening Details (SC06), an
exact full Help action/key/reason census reconstructed from received scrolling
rows (SC16), and independent 1/4/8-row arbitrary-content height fixtures (SC10).
The height oracle adds one title row and two borders, then applies the declared
maximum and margins; it never reads DialogFrame's natural-height calculation.
SC19 establishes Record focus while visible, then narrows with Menu open before
testing restoration after the origin becomes unavailable.

Legacy SC08/SC09/SC15 retain a decoder/encoding prerequisite: the nominated encoder
emits `ESC[27;5;13~` (modifyOtherKeys), which pinned crossterm 0.29 rejects. This is
not labelled "not representable in legacy": explicit CSI-u is acknowledged even
with legacy flags. Each journey proves and captures a Menu/Tab/Enter fallback
before retaining the direct-chord missing-ACK result. A separate bounded real-PTY
diagnosis confirmed that only this chord is dropped; separately written `a` and
Down are acknowledged and Quit succeeds. No decoder repair is part of the sample.
