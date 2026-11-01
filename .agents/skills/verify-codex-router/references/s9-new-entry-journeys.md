# S9 authored journeys and native bindings

The 31 B flows adopt the
four-document TUI component design at
`76803a480b21cad31dc32db1b6fbc86c31aab61b` (including the Open-only correction
at `014e25e08`) and later direct Lead corrections. All cases are historically
`absent-at-base` at `e23f28aff392d7c09905fe65ae64628ddaba81a4`, awaiting S6/S7
entries and their real fixture seams. Native adapters are admitted separately.
An absent entry is neither a product pass
nor a product failure. The Lead declares the later exact target before a run.

The later [Ghostty input harness extension](ghostty-input-harness.md) adds a
separate layer-C keyboard/pointer matrix to `journeys/flows/flow-catalogue.ts` output. It uses the
same existing PTY owner, opt-in input profiles and actual modes; its authored
matrix and apparatus qualification add no product passes to the authored cases.

From the repository root:

```sh
node scripts/verification/tui/journeys/flows/flow-catalogue.ts --check
node scripts/verification/tui/journeys/flows/flow-catalogue.ts --list
pnpm --dir scripts/verification/tui run typecheck
pnpm --dir scripts/verification/tui run lint
pnpm --dir scripts/verification/tui run format:check
pnpm --dir scripts/verification/tui test
```

Both CLI modes validate the authored definitions and print their coverage state,
fixture prerequisites and admission inputs. They never launch a TUI and always
report `kind: authored-not-driven`, `productPasses: 0`. `--list` intentionally
retains the same complete disclosure as `--check`.

## Authored cases

| File under `scripts/verification/tui/` | Cases and deciding outcome | Entry awaited |
| --- | --- | --- |
| `journeys/flows/toolbar-menus.ts` | Sessions and Quota top rows, F10/click, Tab reaches menu, live filter, right-side drop-off with every lost action retained in menu; no bottom action row. Four grids, both key modes. | S6 picker, S7 quota |
| `journeys/flows/claude-details.ts` | Stopped native Claude Details has Open and no Resume in background. Open returns the captured source-qualified target. Four grids, both key modes. | S6 |
| `journeys/flows/source-qualified-selection.ts` | HostedCodex Resume/Fork retain the complete selected service/endpoint/session identity among same-ID rows. Four grids, both key modes. | S6 |
| `journeys/flows/new-dialog.ts` | Open dropdown + kitty Ctrl+Enter keeps committed value and starts once; legacy Tab/Enter and click each remain available. Four grids. | S6 preparation recorder |
| `journeys/flows/account-login.ts` | Re-login-required primary and exact displayed/routed command; captured A survives browse B/reload; command after terminal restore; fresh same-ID success, failure and exit130 cancellation. Both key modes. | S7 command/result fixture |
| `journeys/flows/account-actions.ts` | Every command, exact captured target/root, clipboard full ID, Options/Floor, Disable/Enable, removed-target reasons. Remove only when disabled; Cancel primary/initial focus; Ctrl+Enter dispatches zero; explicit Remove selects former position, last survivor or none when empty. RequiresDisabled refusal never retries. Four grids, both key modes. | S7 command/clipboard fixtures |
| `journeys/flows/quota-refresh.ts` | Key and menu routes, causal hold plus duplicate acknowledgments, one invocation, visible pending/unavailable reason; all-success, partial, all-failed and setup/read error preserve this invocation's typed outcomes and fresh table/open Details. Both key modes. | S7 real CLI adapter with fixture provider |
| `journeys/flows/option-navigation.ts` | Option arrows page and jump to first/last in picker, quota and Help, with exact input delivery and stable semantic selection. Four grids, both protocols. | Declared consumer targets |

`journeys/flows/flow-definition.ts` owns the strict authored schema and pinned references.
Steps name semantic targets/checkpoints, not invented screen coordinates. A
fixture operation names an adapter contract, never a product mutation hook;
an operation without a registered fixture binding remains an admission gap.
The four sizes are 120×36, 76×30, 48×12 and 32×24. Login/refresh
logic cases also use all four grids. The current data declares 212 valid
grid/protocol variants; compound-only flows and separate legacy fallback flows
retain their own protocol restrictions.

## Bind after S6/S7, then observe

Use the existing [compiled capture route](tui-journeys.md), pinned wterm 0.5.7,
Rust PTY owner and original PNG sink. No new renderer or alternate engine is
introduced here. Translate semantic steps into that route only after inspecting
the real delivered entry and its action/fixture contract. Preserve exact source,
artifact hashes, key mode, grid and same-child resize evidence. The present draft
has an interpreter in `journeys/flows/flow-executor.ts` and native runtime in
`journeys/flows/native-flow-runtime.ts`; schema validation cannot turn coverage green.

`journeys/scenes/public-entry-scene-checks.ts` holds reusable scene/projection checks. Stopped-Claude checks
scope to active Session details, so underlying Open text cannot supply its
missing action. Closed action inventories reject omissions, additions and wrong
keys; a toolbar prefix alone is insufficient without menu parity.

`journeys/scenes/effect-checks.ts` defines **verification observation** schemas, not product
wire formats or new product producers. Bind them to independently retained
command, kernel termios, service-return, fixture and rendered observations. For
Refresh, retain the actual CLI adapter and service/provider interaction: a mock
refresher would bypass the mapping under test. Per-account reasons and source
Ok/all-failed/setup-read-error are distinct inputs; stdout and old stored rows
cannot supply them. Hold/release and duplicate input acknowledgments are causal
events, not delays. The focused tests qualify oracle sensitivity only.

For New use the [returned-outcome and input reader recipe](input-consumption-readback.md). Local/default New requires one real returned new request, with exact client, machine, directory and foreground/backgroundThenAttach run mode; no separate preparer count. Configured-source New retains the counted preparation stand-in. The three activation routes cover each of default/local/configured (nine U5 flows). Select source inside the real New Machine control after Ctrl+N. LocalCodex requires exactly its one committed Machine option; hosted/configured routes retain the distinct uncommitted highlight. Configured setup commits the remote then uses Up to highlight the default without picking, because Down clamps at the last option. Missing/duplicate outcomes, wrong source/client/run mode or committed unpicked highlights must fail. These bindings remain preparation until observed on the separately declared S6b executable.

The [source-qualified outcome readback](source-qualified-outcome-proposal.md)
was adopted by Lead push `01a18ce0-eea3-7eb3-b6bb-18b1df4f05d5` for the
Implementer's S6 brief. Its producer remains absent at base. It is test-support
only, produced from the real returned action and captured identity, with same-ID
origin/service/endpoint controls. Keep it separate from preparation count/request.
Actual stopped-session saved settings and attachment are F observations under
the separate [Claude runtime recipe](claude-background.md), not claims from B.

## Settled picker menu census

Lead push `01a18ce5-8c4d-7193-92c9-f1452715b35d` and R13 at `76803a480`
settle all 19 base Help rows. `journeys/flows/picker-action-inventory.ts` retains their
independent source census and classifies each one:

- Nine actions belong in the menu and Help, with the settled names and keys.
- Scope (Ctrl+S), runtime view (Ctrl+T) and sort (Ctrl+O) expand to checked
  value items in both. Scope is This folder/All; retained runtime values are
  All/Blocked/Active/Idle, sort updated/created. Exactly the current value is
  checked in each setting, and every value carries the setting's key.
- Seven input conventions stay under their own Help heading, never in the menu:
  selection arrows, paging, first/last, search syntax, typing/Backspace, the Esc
  ladder and Esc closing Help.

Menu entries (actions plus checked settings) equal Help's action and setting
entries, with the same names and keys. New background session… is present only
for capable clients. Menu F10 belongs to Help and the toolbar, never its own
menu. `checkPickerProjections` rejects a self-opening item, misplaced convention,
lost convention or changed key; `checkPickerSettings` checks the closed values,
keys and current mark. Pass independently declared expected settings and
canonical action names; visible toolbar sigils/short labels have separate grid
checks. Never derive the expected inventory from the implementation under test.
The explicit R13 names Details/Open/Fork/New session…/Help/Quit replace
descriptive base phrases, retaining action identity and keys.
Quota's core seven menu entries are enumerated in R13; R17 adds Log in again and
R18 adds Refresh R. Keys not fixed in R17 await the delivered action definition
and must agree between its menu, Help and toolbar projections.

## Gaps and return

Report each case as authored/absent until the real route has run. Return new
target receipt paths, exact commands/exits, original images and negative-control
outcomes when S9 runs. Offline oracle tests, a fixture stand-in and a returned
action receipt do not establish native/provider runtime effects. Public F,
physical terminals and S5 wrong-build observations remain separate obligations.

## Native authored-flow executor

From the repository root:

```sh
node scripts/verification/tui/journeys/flows/authored-flow-run.ts PLAN.json --check
node scripts/verification/tui/journeys/flows/authored-flow-run.ts PLAN.json
```

`authored-flow-plan.ts` declares separate product/observer source snapshots and
canonical digests, signed driver/product and pinned Node artifacts, Chrome, fresh
output, one authored journey and its grid/protocol matrix. The embedded
`expectationPreflight` derives every event Display, modifier, surface, target and
action from hash-pinned source with file:line references, and records comparisons
with retained receipts wherever those inputs were reached. Missing/incomplete
matrix preflights reject before launch. `--check` qualifies these sources,
artifacts and emulator and reports zero product passes.

`picker-flow-bindings.ts` binds `stopped-claude-details-open-only`, same-ID
HostedCodex Resume, and nine New flows through source-pinned adapters. Resume
selects Indigo, independently distinguished from Quartz by service and Cedar by
endpoint. New uses `new-machine-uncommitted` or `local-codex-new`, the actual
dropdown, and returned or counted preparation readbacks. A terminal Start click
requires its MouseDown ACK and real exit/outcome, with no release sent after
the product loop ends and no promised continuing frame. Pointer preflights name
the independent label and ownership; actual coordinates come from the unique
fully painted target in the prior marked frame, never invented pre-launch cells.
Its first overlay identity is independently derived from fresh-stack source,
never learned from the ACK under test. Other adapters remain named admission
gaps before startup. On the six-scenario S6b declaration, Fork needs
`fork-ssh-profile`, toolbar's successful Claude New needs `claude-new-success`,
and the two >2-page alias journeys need `large-list`; the base six-row fixture
does not supply these. The interpreter accepts all 31 data definitions; that does
not mean all 31 have fixture bindings.

Each press/focus hop is sent alone and awaits its exact receipt and marked frame
before the next step. Adjacent contiguous markers may share one just-completed
frame for a burst, which proves only state after the batch's last input.
Type/click batches record `after-burst` evidence. Intermediate states require
separate authored steps. Missing, inside-frame, duplicate and skipped markers
still reject; later unmarked frames cannot replace the bound frame. Loop-ending
inputs settle through exact ACK, real EOF, outcome and exit status; their marker
is optional.

Async inspection/source publication waits for the exact ready scene, then uses
a harmless F12 ACK/marker for capture. A setup ACK, arbitrary redraw or delay
cannot supply publication proof. Capture keeps the marked native prefix, grid,
frame binding and original wterm PNG; the full transcript retains the marker
following that image prefix.

LocalCodex has one Machine option. Lead correction 01a19527 requires that single
committed option and preserves all its other New obligations; distinct-highlight
coverage belongs to multi-machine sources. Pure permutation uses the ordinary component showcase's
`ComboBoxControl.replace_options`; picker sources sort provider IDs and support
add/remove/replace with index shifts instead.

Account actions run in process; terminal restoration/subprocess argv applies
only to login. Options/Floor replace Details, so after Esc the captured-A/B route
reopens A through real Enter and changes Browse to B under that captured dialog.
Disable/Enable retain Details and get no extra Enter. Setup control ACKs remain
distinct from actual UI input/frame proof.

The catalogue exposes named `sessions-group-by-f6` and `quota-group-by-f6`
placeholders for U18/R19 while the grouped crate/consumer target is pending.
F6 is the settled grouping key on both screens; picker Ctrl+G/F2 remain Machine.
A placeholder is not an authored runnable flow or a product pass.
