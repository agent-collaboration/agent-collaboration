# Sessions fixture receipt transport

Lead decisions `01a18dc6-435f-7f43-b1c6-b47cd8b13b69` and
`01a18dca-0e28-7c52-99d2-14004c8a640a` authorize one optional launch input:
`TUI_SESSIONS_FIXTURE_RECEIPT_DIR`. Only the verification fixture entry reads it;
product code gets typed per-instance sinks. Normal/release has no switch.

The Verifier owns forwarding and readers; the Implementer owns S6 producers.
The nominated directory receives `picker-outcome.json`, `input-dispatch.jsonl`
and the separate preparation receipt. Missing input means no receipt writes,
never a fallback. Receipt contents remain unobserved until real S6 bytes run
through their readers and controls; transport qualification is not product proof.

## Two launch paths

- For the TypeScript `fixture-child.ts` sessions wrapper, explicitly put the
  variable in the wrapper's launch environment. `main` forwards it through
  `childLaunchConfiguration`. It is not inherited by quota/component children
  or public Sessions. The launch record itself has no new field.
- For the compiled grid route, set `PtyGridTarget.sessionsReceiptDirectory`.
  `PtyGridDriver` suppresses an ambient variable and forwards only that explicit
  nomination. `grid_terminal_capture.rs` passes it to the existing
  `TerminalDriver::spawn_with_environment` for `sessions` and `sessions-filters`.
  The owner allowlist admits exactly that key. Other fixture surfaces reject a
  nomination. Existing callers omit the option and retain their old behavior.

Create a fresh, empty receipt directory directly under the capture's owned
output/evidence directory before launching. The TypeScript boundary rejects a
relative, missing, non-directory, symlinked, foreign-owned, outside or reused
path. The Rust grid boundary requires an absolute, existing, non-symlinked empty
directory. Neither transport creates receipts or chooses a substitute directory.
Use one directory per retained child; never reuse a finished child's receipts.

Qualification:

```sh
pnpm --dir scripts/verification/tui exec vitest run sessions-receipt-directory.test.ts
cargo test -p codex-router-quota-reset-test-harness --test quota_storyboard_capture_test sessions_ -- --nocapture
```

The TypeScript checks exercise fresh-path rejection, absence, quota isolation
and a real child process observing the explicit grid-driver input. The Rust
checks cross the real PTY environment boundary and real wrapper/child path on
the existing owner, including absence and quota isolation. Controlled children
record environment only; they don't claim picker outcomes, input routing or
preparation effects. Readbacks follow the separately adopted
[source-qualified outcome](source-qualified-outcome-proposal.md) and
[causal input acknowledgement](input-consumption-ack-proposal.md) contracts.
