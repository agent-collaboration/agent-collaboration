# Native test cutover inventory

The A3c checkpoint is `a34928fe`. This follow-up closes the unfinished A3a
existing-tests row. Native registration answers below are explicit fixture
inputs, persisted and interpreted by the actual library writer. They do not
prove central policy or enrollment.

| Old case or surface | Obligation | Current proof and status |
|---|---|---|
| Aggregate CLI pages and batch acknowledgements | Project keys stay independent; expiry writes nothing; same-key replay survives renewal | Same case, real native registration expiry and renewal. Passed, included in the 12-case CLI run below. |
| MCP disconnected mutation waiting on the store | A disconnected TCP/Unix caller leaves no project mutation while blocked by the writer | Same case, holding the native `begin_write` guard. Passed in the 10-case API group. |
| CLI model, fleet lifecycle, task corrections, repository subjects and role/subject parity | Actual executable and MCP admission preserve domain outcomes, IDs and visibility | Same cases and assertions, with actual native registry construction and explicit origin. Eight CLI targets passed 12 cases, zero failed/ignored, exit 0. |
| CLI subscribe, wait and cancel | Membership and cancellation pass through the executable, listeners and actual storage | Same scenario, native registry. Passed in that CLI run. |
| CLI/client overload | A shed request has no effect and remains retryable | Native constructor replaces fake authority. CLI passed; client group passed 11 / 0, exit 0. |
| MCP subscription entry | Typed MCP tools use actual admission and storage | Native constructor replaces fake authority. Full MCP lib run passed 109 / 0, two explicitly ignored, exit 0. |
| Cleanup while the Host is live | An explicit unsubscribe leaves no delivery-eligible subscription | Public `thread_subscriptions` observation. Cleanup group passed 12 / 0, exit 0. |
| Cleanup exact terminal readback | The same membership is `Ended` with `Cancelled` reason and zero pending work; watch and automation state persist | Exact assertions run after Host shutdown through native reopen, retaining Ended/Cancelled and zero-pending checks. Cleanup group passed 12 / 0, exit 0. |
| `cursor_loss_starts_at_the_ownership_floor_and_preserves_inherited_intents` | MS-11 / PD4 C35: successor continuity after local progress loss | Retired with Lead authorization: synthesized writer epochs, feed checkpoint and fake lease authority are a dead contract. The A6 successor must rescan inherited handoffs above the replicated settled frontier and prove deduplication through the real relay. **Unverified until A6.** |
| `returning_holder_gets_a_new_floor_across_its_unbroken_epoch_run` | MS-11 / PD4 C35: continuity when a machine holds the project again | Retired with Lead authorization: no ownership-floor contract remains. The A6 successor must continue inherited handoffs under their original IDs after re-hold, without duplicate delivery. **Unverified until A6.** |

No compatibility lease APIs, ignored replacement cases or raw SQLite readback
were added. The two retired cases are the only removed tests in this follow-up;
their private helpers and module registration were removed with them. No A6
successor passing result is claimed.

The cleanup change preserves the durable terminal assertions and moves their
observation after stop, as Verification Design §6 requires. A public absence
while live is not relabeled as an exact terminal state.

Validation: the MCP API group passed 10 / 0; eight selected CLI targets passed
12 / 0; cleanup passed 12 / 0; client overload passed 11 / 0; full MCP lib
passed 109 / 0 with two explicit external/live ignores. All commands exited 0.
All-target Clippy for agent-collaboration, collaboration-client, collaboration-mcp
and collaboration-service passed with warnings denied, exit 0. The workspace test compilation passed: cargo test --workspace --no-run --locked
--offline, exit 0, 591 test executables, 726.97 seconds including the disk/source
guard. All 3,857 source hashes matched. Free space stayed above 45 GiB and was
93 GiB at completion. This compiled the test targets; it did not execute the
whole workspace suite. The existing Apple compact-unwind warning remains.
The command ran with HEAD 51f1bfd6 plus this A3a working-tree delta. The source
manifest binds that observed product state to this commit; only proof documents
changed after the run. No post-commit rerun is implied.

The actual-producer runtime case, known-prefix cleanup cases, unavailable-socket case and failure projections all passed in the 12-case cleanup group. No native registry is opened while the Host is live; public roots and eligibility are inspected through its API, then the stopped store supplies the exact durable terminal observations. The normalized corruption tests also passed in the 14-case participant group; each intentional row mutation asserts its exact affected-row count and retains its typed InvalidRecord oracle.

The full MCP lib run first found two additional stale fixtures: owner-grant actors still used the retired wrapper, and the manual post sample still put origin in the actor and used old position/time shapes. These samples now use the adopted actor, origin-bearing position and RecordTime wire forms. Assertions and the 44-operation/28-tool parity matrix remain intact. The rerun passed 109 cases, zero failed, two explicitly external/live cases ignored, exit 0.


The first workspace attempt included an unsupported --keep-going flag and
stopped before compilation, exit 1, with no failing crate and unchanged source.
The successful command above removed that flag. No backward-compatibility
lease mechanism, proof waiver or production process change was introduced.
