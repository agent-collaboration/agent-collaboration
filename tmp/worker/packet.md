# agent-sessions UX fixes (Sol worker — run ALL builds/tests on Sunclaw)

Owner, 2026-10-29: "more slop" (Client action dialog screenshot); "multiple problems. 1. keyboard navigation stops working. 2. in dialogs main submit should be ctrl enter. 3. opening in background a claude agent doesn't even work, does this weird thing, opens background agent, doesn't attach"; "new claude agent starts background agent, doesn't do anything".
Branch fix/sessions-ux-claude-attach from main (0.1.116). One PR. Version: next unused at merge (Lead assigns).

## 1. Claude open/new must land the user IN the session (behaviour bug)
Today crates/agent-session-inventory/src/session_commands/claude_launch_target.rs:116-129 always runs `claude --bg [--resume <id>]` and returns: new agents sit idle in the background; opening a session already running in the background starts a COPY. Installed Claude Code 2.1.312: `--bg` "starts the session in the background and returns immediately; prints the id that claude attach … take"; `claude attach <id|name>` "opens the background session in this terminal".
Required:
- New: `claude --bg …` → parse the printed id (from the CLI's actual output; confirm format against 2.1.312) → replace the process with `claude attach <id>` in the same terminal.
- Open, session already running in background (catalogue ProviderSessionSummary::ClaudeCodeBackground with job handle; state Working/Blocked): `claude attach <job>` directly — no --resume, never a copy.
- Open, not running: `claude --bg --resume <id>` → attach to the printed id.
- Keep Router routing env/auth exactly as today (routed_environment).
- Tests: argument/flow selection for the three cases (fail on current main for new + already-running), plus id-parsing from a recorded 2.1.312 output.

## 2. Client action dialog (crates/agent-session-picker/src/session_picker/client_action_view.rs ~105-160)
No Rust Debug formatting anywhere in user text (`{status:?}`, `Resume stage: {:?}; effect: {:?}`, `Operation stage: {:?}…`). Map each typed outcome to a plain sentence + next step (exited normally / exited with code N / stopped by signal; resumed / loaded / pending — check the operation; failed: <message>). Do not paste raw CLI stdout/stderr; on failure show at most a short labelled excerpt. Layout with iocraft components (AGENTS.md Terminal UI Layout), no padded strings. Test: render every outcome; assert no `{`, `}`, `::`, `Some(` debug artifacts.

## 3. Keyboard navigation stops working
Reproduce first (likely after the Client action dialog closes or after a native child process exits/returns: focus or raw-mode/keyboard state not restored). Find the cause; fix at the owner (focus/terminal-mode restore). Test that reproduces it (fails on main).

## 4. Ctrl+Enter is the primary submit in every dialog (owner decision)
Enter keeps its in-field meaning (select/move); Ctrl+Enter triggers the dialog's main action everywhere (new session Start, filters Select, floor editor Save, etc.). Enable crossterm keyboard enhancement (disambiguate escape codes) so Ctrl+Enter is distinguishable where the terminal supports it (Ghostty/kitty/WezTerm); show "Ctrl+Enter" in each dialog footer; where the terminal can't report it, the primary button remains reachable via Tab then Enter. One shared key-handling path for all dialogs (no per-dialog duplicates). Tests per dialog.

## Proof (on Sunclaw)
fmt; clippy -D warnings (touched packages, all targets/features); nextest for agent-session-inventory, agent-session-picker, codex-router-tui, agent-collaboration; release check; one real journey with the signed debug build on Sunclaw: new Claude agent attaches; opening a running background session attaches to the original (no copy, `claude agents` shows one); dialog text clean; keyboard nav works after the dialog. Debug Router tests use claude-haiku-5-5, small budget.

## Rules
Report only to the 🦁 Lead (claude-local 9f6bdec4-853f-4d3a-8ed0-0d7b4b128af3 on Sunbook service 0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89) via fleet message. No builds/tests on Sunbook. Never touch production Routers/Hosts. No forks/pins, no Python, no file >1,000 lines, no `allow`, no git stash/amend/--no-verify; commit --no-gpg-sign. No wall-clock tests (wait on signals/injected clocks). Design break → stop and report.

## Additions (owner screenshots, 2026-10-29) — do item 0 FIRST
0. **Keyboard navigation doesn't work at all** (owner: "keyboard navigation doesn't work at all"), in the session list and in dialogs (e.g. New session: Client/Machine/Directory/Cancel/Start unreachable by keys). Reproduce with the installed 0.1.116 and on main; find the cause (key events not delivered/handled, focus never set, keyboard-enhancement/raw-mode state). This is P0 for this PR.
5. **Dialog overlay doesn't cover what's under it**: in the New session dialog the list rows bleed through the dialog's left edge/border ("C", ✉, 📋 glyphs drawn over the border column), and the dialog body is mostly empty (oversized). The dialog must paint its full rectangle (background + border) over the list and size to content (with the existing min/max rules). Component-level fix in the shared dialog frame, not per dialog. Render test that fails on main (underlying glyphs inside the dialog rect).
6. **Untrusted workspace**: new Claude agent in a directory Claude hasn't trusted fails under `--bg` with exit 1 "Workspace not trusted…" (seen on Sunclaw source, /Users/shravansunder/code). `--bg` can't show Claude's trust prompt. Required: detect this case (from the CLI's actual result, not a guess) and give the user a working path: for a LOCAL session, start Claude interactively (no --bg) in that directory so the trust prompt appears, then continue normally; for a REMOTE source (another machine), report the options with evidence (e.g. attach via the existing source connection with a TTY, or a clear message with the exact command to run on that machine) — STOP and report before building a remote attach mechanism; the Lead decides.
Also: remote (other-machine) open/attach in general — report how item 1's attach should work when the session's source is another machine, with evidence of what the current source connection can do; do not build it before the Lead decides.

## Owner standards for every TUI dialog (2026-10-29, verbatim intent: "the entire flow should be mouse and keyboard drivable … both should work, keyboard should work; ctrl enter should be accept main action of dialog, this should be standards; esc is escape in dialog component; arrows should work")
Implement ONCE in the shared dialog component (codex-router-tui DialogFrame / FormFields / SearchSelect / SelectableRow layer), so every dialog inherits it; no per-dialog key handling:
- Entire flow drivable by keyboard AND by mouse (click/scroll) — list, dialogs, pickers, buttons.
- Ctrl+Enter = accept the dialog's main action (Start/Select/Save/…), everywhere.
- Esc = escape: cancel/close the dialog (or close an open dropdown first, then the dialog), everywhere.
- Arrow keys move between fields/options/rows; Enter keeps its in-field meaning (open dropdown / choose option); Tab/Shift+Tab also move focus.
- Footer shows the keys.
Also add to AGENTS.md "Terminal UI Layout" exactly: "Every TUI flow is fully keyboard- and mouse-drivable. In every dialog, Ctrl+Enter accepts the main action, Esc cancels or closes, and arrow keys move between fields and options; implement these once in the shared dialog component."
Tests: per shared component (key → action) and one per dialog proving inheritance; a mouse-click path for list + dialog buttons.

## 7. Remote launch terminal environment (owner: "something weird with ssh or colors or display")
The remote source path runs `ssh -tt … exec <agent-sessions> --source-request '<json>'` (source_ssh_transport.rs:35-66). ssh forwards TERM with the pty but not COLORTERM/TERM_PROGRAM (unless SendEnv/AcceptEnv). Verify on Sunclaw what the launched client (codex/claude) actually receives (TERM, COLORTERM, TERM_PROGRAM, LANG, tty size) versus local; if they differ, carry the caller's terminal env explicitly in the source request (allow-listed keys only) and set it for the launched client. Evidence first; report if a design choice is needed.

## 8. Sessions screen header and focus (owner screenshot 2026-10-29: "I don't know what this details thing is; help can't be navigable by keyboard")
- Focus model: on open, keyboard focus is ON THE LIST with the first row visibly selected (❯ marker + row highlight). Arrows move rows; PageUp/PageDown/Home/End work; the header actions (New, Details, Help) and the two filters are reachable by Tab/Shift+Tab and by mouse click, and show which one has focus. Today no row is selected and arrows don't reach the list.
- "Details": it opens the selected session's details (details_dialog_state.rs). Make that obvious: label it for what it does (e.g. "Session details"), show it as an action of the selected row (also on Enter/→ or its shortcut), disabled with a reason when no row is selected. If you think it should be removed or merged instead, report with a screenshot and reasoning; don't remove it on your own.
- Help: opens by keyboard (its shortcut + focus+Enter) and mouse; the Help dialog scrolls with arrows/PageUp/PageDown, Esc closes, and it lists the real current shortcuts (generated from the same key map, so it can't drift).
- Same standards as all dialogs (keyboard+mouse, Ctrl+Enter, Esc, arrows). Proof: keyboard-only and mouse-only journeys through list → New → Details → Help on the signed debug build on Sunclaw, with captures.
