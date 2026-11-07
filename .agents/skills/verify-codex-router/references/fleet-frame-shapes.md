# Fleet frame shapes

An identified invalid request or answer body must return the correlated
`InvalidShape { field }` while the same authenticated socket remains usable.
An unreadable kind or frame ID closes as `MalformedFrame` without routing.
The baseline is agent-fleet-sync b9470dc's SO56 socket controls, now in
`crates/fleet-native-services/tests/e2e/native_frame_shapes.rs`.

Use the target, signing and slot checks in
[fleet native consumer](fleet-native-consumer.md). Run from the repository root:

```sh
env -u GITHUB_ACTIONS CARGO_BUILD_JOBS=2 cargo nextest run --locked -p fleet-native-services --features test-support,test-fault --test e2e --test-threads 1 --status-level all --final-status-level all -E 'test(/^native_frame_shapes::/)'
```

This corrects the old standalone `--test native_frame_shapes` selector: the
source was under `tests/consumer/tests/integration/native_frame_shapes.rs`,
and the destination is the **e2e aggregate's module**. Require these seven
identities to execute; zero matches never pass. If the complete native suite
already ran on the same qualified target, retain its observations instead of
repeating an equivalent selection.

| Case under `native_frame_shapes::`                                         | Deciding observation                                                                                                                                         |
| -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `identified_bad_bodies_are_correlated_and_the_same_session_stays_usable`   | Exact `text`, `body`, `unexpected` or unnamed-body fallback; oversized name capped at 64 Unicode characters; five valid follow-ups route on the same socket. |
| `unreadable_kind_or_frame_id_closes_as_malformed_without_routing`          | Malformed JSON, invalid/missing ID and unknown kind close separate connections with zero routed calls.                                                       |
| `unknown_fields_with_backticks_keep_their_complete_correlated_names`       | Whole embedded/leading/diagnostic-like names and matching frame IDs, followed by three valid same-socket calls.                                              |
| `unknown_fields_choose_the_actual_empty_or_nonempty_diagnostic_suffix`     | Six exact diagnostic-looking names/frame IDs survive, with six valid same-socket follow-ups. The historical name does not authorize parsing diagnostics.     |
| `unknown_fields_are_not_confused_with_declared_names_or_nested_input_keys` | Scalar/nested and content-before-kind unknowns name the actual field; four valid same-socket follow-ups route.                                               |
| `discarded_backtracking_errors_do_not_replace_the_final_reported_field`    | Discarded untagged candidates cannot replace `outerAfter`; unnamed errors fall back to `body`; only two valid follow-ups route.                              |
| `missing_and_duplicate_declared_fields_keep_their_exact_names`             | Complete declared names, matching frame IDs and two valid same-socket follow-ups.                                                                            |

Inspect `crates/fleet-session/src/frame_shape.rs` and its caller for structured
Serde field capture, unnamed fallback, Unicode cap and restoration of discarded
candidate errors. Diagnostic-text parsing, recursive input matching and parser
position guesses are wrong behaviors the socket assertions must catch.

When entity-key admission is in scope, retain the separate e2e module
`native_engine_identities`: a deliberately invalid UUIDv4 seed entity is refused,
its valid follow-up uses the same session and no seed receipt is committed.
This negative fixture does not permit generating ordinary UUIDv4 identities;
new normal identities follow the repository's UUIDv7 rule.

Wrong-build controls require a separate fault grant. A compiler or fixture
failure is not sensitivity to a wrong returned field. Keep mutation bytes,
actual mismatch, restoration hashes and the restored positive observation.
Report raw Rust case counts separately from frame subcases. Confirm owned
children settled and retain evidence as in the native recipe. A reconnect does
not satisfy same-session recovery; local sockets establish no hosted session
or delivery acceptance.
