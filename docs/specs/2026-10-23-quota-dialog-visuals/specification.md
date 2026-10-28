# Observable visual behavior

[Requirements](requirements.md) is the authority for this visual delivery. Accepted set: U1–U8 and its preservation boundary. Existing component-system obligations remain where this document does not explicitly supersede presentation.

## Capability and entity model

Quota screen owns R1–R6; Filters and dialogs owns R7–R10. R9–R10 also serve Quota. A person browses a Surface, focuses a Row, and opens its Dialog; filtering or resizing must not change the underlying identity of that Row or captured Dialog action.

| E · term | Same-instance rule and cardinality | Invariants / states / basis |
| --- | --- | --- |
| E1 Surface | One mounted Quota or Sessions view across resize; displays many Rows and at most one active Dialog | Browse/obscured/restored; existing source and controller retained. U1/U4/U8. |
| E2 Row | One source-qualified account/session identity, independent of title, order, query or width; belongs to one Surface | Selected/unselected changes appearance, never identity or left edge. U1/U3/U8. |
| E3 Quota window | Account identity plus window duration; a newer observation updates the same account/window, not its name | Known/unknown/stale and reset known/unknown remain distinct. U2/U5. |
| E4 Dialog | One current interaction and its captured account/session target or filter draft; belongs to an opener Surface | Open/editing/pending/error/closed; no stacked active dialogs or effect retargeting. U6–U8. |
| E5 Choice field | One purpose within a Dialog; choices have existing stable keys | Query and focus are not committed selection; closed/searching/no-match. U6/U8. |

## Contracts

P1: overcrowded/unclear presentation shown in owner screenshots. O1: compact information hierarchy. O2: attributable actions and preserved semantics. Each R has its corresponding C observable contract and V proof obligation in the same row. The reference is the owner's accepted study and these literal worked forms, not current implementation output.

| R · scenario | C · obligation, including negative case | U / E / O / V |
| --- | --- | --- |
| R1 Read account | C1: wide browse has Account / Quota / Weekly pace / Extra, in that order. Account shows full name, then provider+client count, then one status. No separate Status column and no balance block beneath pace. Long names may wrap; the full identity remains inspectable. | U1; E1/E2; O1; V1 |
| R2 Read quota | C2: included windows share Quota as `bar 92% · week · 6d 16h` and `bar 83% · 5h · 2h 10m`, with header “Quota left · reset in.” Repeated per-line “Resets” is absent. Missing window shows No data and no full/empty fabricated bar; unknown reset shows a dash or explicit unknown. | U2; E2/E3; O1; V2 |
| R3 Distinguish windows | C3: weekly uses white, 5h a slightly softer cool white, with window text remaining readable. Stale warning remains semantically distinct; known amount never becomes unknown merely because reset is unknown. | U2/U3; E3; O1; V3 |
| R4 Read headings and selection | C4: bold white column/group headers have a distinct background and teal rule. Alternating account backgrounds are subtle; selection has its own stronger style plus yellow marker/name. Hover must not overwrite selected appearance; selected/unselected content starts stay fixed. | U3/U8; E1/E2; O1/O2; V4 |
| R5 Fold and separate | C5: width adaptation preserves group order and meaning. Folded Account/Quota/Pace/Extra have no intervening blank row; headings distinguish groups. Whole accounts are separated by two blank rows. On constrained height, content scrolls and actions remain usable; no fact may become permanently unreachable. | U4/U8; E1/E2/E4; O1/O2; V5 |
| R6 Read quiet summary | C6: routine pool availability/freshness census is one action away through labeled Pool details, opened by F5 or its header action from interactive browse. Esc closes it; existing body-scroll keys work. All census facts, including zeros, remain accessible there. One row status is shown; its reason and independent freshness/availability remain in Details/Help. Browse credit text is quantity-first, e.g. 62.5k credits, with explicit stale warning when applicable. Credential-lock/unavailable and operational/projection-degraded errors remain visible in the browse header; routine stale/unknown counts move to Pool details. | U5; E1/E2/E3/E4; O1/O2; V6 |
| R7 Search a choice | C7: visible Search text is followed by the current query and an end-position caret, with nested options below (empty query: `Search: ▏`). Existing search/paste/arrow/Enter/Escape/pointer and unavailable/no-match behavior remain. A cursor cue grants no new input authority or implicit commit. | U6/U8; E4/E5; O1/O2; V7 |
| R8 Inspect a session | C8: readable captured session name and structured labels/values/secondary metadata replace the undifferentiated instruction wall. Full original title, path, ID and available history remain reachable. Display shortening must not persist a renamed session or rewrite original content. | U7/U8; E2/E4; O1/O2; V8 |
| R9 See and activate actions | C9: dialog actions have a distinct footer surface and key/focus hierarchy, outside the scrolling body. Existing eligible Copy/Resume/Fork/Load, floor/credit/reset actions, disabled/pending/error behavior and captured-target guards remain. Key text describes only the actual existing per-surface bindings listed below, plus the new F5 summary input in C6. A label claiming a nonexistent binding, or labeling Enter as a fixed action where Enter activates the focused action, fails. Narrow layout may wrap actions; it must not silently remove an action. | U7/U8; E4; O1/O2; V9 |
| R10 Preserve boundaries | C10: every view is composed in the existing terminal component system, uses a single terminal font size and cell geometry, and leaves domain/persistence/network/process behavior unchanged. Existing static output and JSON contracts remain functional; interactive changes do not force static consumers onto a new format. | U8; E1–E5; O2; V10 |

Each V passes only if its C is observed on a compiled affected target at a relevant wide, compact, narrow and short-height grid, except R10's source/no-effect clauses which also need structural checks. The counterexamples in C2/C4/C5/C7/C8/C9 distinguish wrong behavior; fitting proof cannot be replaced by an HTML-only or mock-only pass. No storage migration or release behavior is part of the reference.

## Presentation versus behavior

New presentation supersedes the older exact Account/Status/Quota grouping, old white-only section headings, old action-placement details, and old browse-header placement of routine census/freshness/problem counts. Other existing component-system/provider workflows remain protected. Newly exposing a pool summary is read-only. Missing/stale data remains explicitly missing/stale. Refresh, rejected choices and pending effects retain their current owners, rather than acquiring new retry/fallback policies.

Real removal and lifecycle/credential deletion are not a Dialog action in this delivery. No sample-only action may be shipped as if backed by a durable operation.

## Adopted reference and actual input labels

The HTML and isolated captures govern composition, grouping, typography, color and spacing only. Their letter shortcuts (`/`, `?`, `a`, `c`, `p`, `m`, `f`), Account actions dialog, Disable/Enable/Remove, More submenu and disclosures are not adopted. Show the full original session title as ordinary reachable body content; no new disclosure state is needed.

| Surface | Actual input contract and footer label source |
| --- | --- |
| Quota browse | Up/Down focus account; PgUp/PgDn scroll; Ctrl+F/F3 Filters; F1/Ctrl+/ Help; Enter/F4 Account details; Ctrl+E Weekly floor when eligible; Ctrl+R Account options; Esc/Q and Ctrl+C/Ctrl+D Exit, preserving existing pending-reset exit guards. Add only F5 Pool details. |
| Pool details | F5/header action opens from unobscured browse; Esc/Close closes; Up/Down/PgUp/PgDn scroll. It never receives account-effect shortcuts or retargets the account selection. |
| Session Details | Left/Right and Tab/Shift+Tab focus existing eligible actions; Enter/Space activates the focused action; Esc closes; Up/Down/PgUp/PgDn/Home/End retain body scroll; Ctrl+Enter Resume and Ctrl+Shift+Enter Fork retain eligibility guards. Copy ID, Copy path, Load and Close are also focused-button actions, without new letter keys. Never display Enter as a fixed Resume binding. |
| Other existing dialogs | Keep each caller's existing actions, keyboard owner and input mapping. Shared footer styling adds no bindings or mandatory shortcut decoration; reset confirmation retains one undecorated No/Yes pair. |
