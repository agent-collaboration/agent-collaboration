# S9 new-entry journeys — authored, absent at base

These B cases are a **draft**, not an operable product driver. They adopt the
four-document TUI component design at
`76803a480b21cad31dc32db1b6fbc86c31aab61b` (including the Open-only correction
at `014e25e08`). All 19 cases are explicitly
`absent-at-base` at `e23f28aff392d7c09905fe65ae64628ddaba81a4`, awaiting S6/S7
entries and their real fixture seams. An absent entry is neither a product pass
nor a product failure. The Lead declares the later exact target before a run.

The later [Ghostty input harness extension](ghostty-input-harness.md) adds a
separate layer-C keyboard/pointer matrix to `s9-prestage.ts` output. It uses the
same existing PTY owner, opt-in input profiles and actual modes; its authored
matrix and apparatus qualification add no product passes to these 19 B cases.

From the repository root:

```sh
node scripts/verification/tui/s9-prestage.ts --check
node scripts/verification/tui/s9-prestage.ts --list
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
| `s9-journeys/toolbar-menus.ts` | Sessions and Quota top rows, F10/click, Tab reaches menu, live filter, right-side drop-off with every lost action retained in menu; no bottom action row. Six grids, both key modes. | S6 picker, S7 quota |
| `s9-journeys/claude-details.ts` | Stopped native Claude Details has Open and no Resume in background. Open returns the captured source-qualified target. Six grids, both key modes. | S6 |
| `s9-journeys/new-dialog.ts` | Open dropdown + kitty Ctrl+Enter keeps committed value and starts once; legacy Tab/Enter and click each remain available. Six grids. | S6 preparation recorder |
| `s9-journeys/account-login.ts` | Re-login-required primary and exact displayed/routed command; captured A survives browse B/reload; command after terminal restore; fresh same-ID success, failure and exit130 cancellation. Both key modes. | S7 command/result fixture |
| `s9-journeys/account-actions.ts` | Every command, exact captured target/root, clipboard full ID, Options/Floor, Disable/Enable, removed-target reasons. Remove only when disabled; Cancel primary/initial focus; Ctrl+Enter dispatches zero; explicit Remove selects former position, last survivor or none when empty. RequiresDisabled refusal never retries. Six grids, both key modes. | S7 command/clipboard fixtures |
| `s9-journeys/quota-refresh.ts` | Key and menu routes, causal hold plus duplicate acknowledgments, one invocation, visible pending/unavailable reason; all-success, partial, all-failed and setup/read error preserve this invocation's typed outcomes and fresh table/open Details. Both key modes. | S7 real CLI adapter with fixture provider |

`s9-journey-definition.ts` owns the strict authored schema and pinned references.
Steps name semantic targets/checkpoints, not invented screen coordinates. A
fixture operation is an unmet binding requirement, never an implemented hook.
The six sizes are 120×36, 76×30, 48×24, 48×12, 40×24 and 32×24. Login/refresh
logic cases start at 120×36; the shared visual cases cover the six-grid matrix.

## Bind after S6/S7, then observe

Use the existing [compiled capture route](tui-journeys.md), pinned wterm 0.5.7,
Rust PTY owner and original PNG sink. No new renderer or alternate engine is
introduced here. Translate semantic steps into that route only after inspecting
the real delivered entry and its action/fixture contract. Preserve exact source,
artifact hashes, key mode, grid and same-child resize evidence. The present draft
has no executor for these steps; schema validation cannot turn coverage green.

`s9-scene-checks.ts` holds reusable scene/projection checks. Stopped-Claude checks
scope to active Session details, so underlying Open text cannot supply its
missing action. Closed action inventories reject omissions, additions and wrong
keys; a toolbar prefix alone is insufficient without menu parity.

`s9-effect-checks.ts` defines **verification observation** schemas, not product
wire formats or new product producers. Bind them to independently retained
command, kernel termios, service-return, fixture and rendered observations. For
Refresh, retain the actual CLI adapter and service/provider interaction: a mock
refresher would bypass the mapping under test. Per-account reasons and source
Ok/all-failed/setup-read-error are distinct inputs; stdout and old stored rows
cannot supply them. Hold/release and duplicate input acknowledgments are causal
events, not delays. The focused tests qualify oracle sensitivity only.

For New use the already adopted strict preparation receipt:
`{callCount:1,request:{client,machine,directory,runMode}}`. The expected committed
request is declared independently; highlighted-but-uncommitted dropdown values
must not leak into it. `pending-s6` is never effect proof.

The [source-qualified outcome readback](source-qualified-outcome-proposal.md)
was adopted by Lead push `01a18ce0-eea3-7eb3-b6bb-18b1df4f05d5` for the
Implementer's S6 brief. Its producer remains absent at base. It is test-support
only, produced from the real returned action and captured identity, with same-ID
origin/service/endpoint controls. Keep it separate from preparation count/request.
Actual stopped-session saved settings and attachment are F observations under
the separate [Claude runtime recipe](claude-background.md), not claims from B.

## Settled picker menu census

Lead push `01a18ce5-8c4d-7193-92c9-f1452715b35d` and R13 at `76803a480`
settle all 19 base Help rows. `s9-picker-action-inventory.ts` retains their
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
