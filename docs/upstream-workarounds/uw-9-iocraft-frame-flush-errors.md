# UW-9: the synchronized frame guard discards end-frame errors

**Status:** in place, 2026-10-31
**Dependency:** iocraft 0.9.2, fork revision `829ebd86bd07fe2100c096ec6d785875ec2e0c79`

## Lives in

`agent-collaboration-tui`'s `dispatch_frame_marker.rs` and
`InputDispatchReceiptSink::flush_and_check`, enabled with `test-support` and
debug assertions. The fixture binds one per-owner sink to InputOwner and its
chosen render stream.

## Problem and evidence

At the pin, `terminal.rs:355-369` ends a synchronized frame from
`SynchronizedUpdate::drop` with `let _ = self.inner.inner.end_frame()`.
Consequently the awaited native render loop does not report an error returned
by the backend's end-frame flush. A verification fixture could finish with a
successful receipt-file check even though its dispatch marker was not written.

The permanent component test
`frame_flush_failure_is_retained_for_the_fixture_when_the_pin_discards_end_frame_errors`
arms a writer fault after the initial frame, routes F8 through the actual
InputOwner and native render loop, and proves that the end-frame flush was
attempted. Before retention, run `7a06e783-17f5-466f-bf65-2acb8d2c5175` failed
because `flush_and_check` returned `Ok(())`; it was an immediate assertion,
without an entry or redraw timeout.

## Carried fix

The dispatch frame writer retains its first I/O failure in the existing
per-owner frame state, while returning that error through `Write` as usual.
The receipt sink's existing final check also reports this retained failure.
Repeated checks preserve it. The writer emits a marker only after the
underlying frame flush succeeds.

## Recreate and removal check

From the workspace root:

```sh
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui --features test-support
```

The named native-loop fault test must report the retained error; the
`dispatch-marker-forgets-output-failure.patch` control must fail that assertion.
Recheck after an iocraft upgrade. Retention can be reconsidered once an
end-frame failure reaches the awaited render-loop result and the real fault
journey fails closed without this guard. Preserve marker ordering, no-op,
terminal-outcome and byte-identity checks when changing that integration.
