# Legacy Fleet removal inventory

Source anchor: `1aea1301292ef847d8f4c277899adb520c9d5875` (`feat/central-one-core`). Source-only inventory commissioned by the Lead in Router message `01a15321-52b0-75f1-a4ce-f5d0adf16c8e`, after C25 component acceptance. No test, request, implementation, migration, API or mechanism is changed by this inventory. This is a bounded candidate/disposition checklist, not proof of passing tests or permission to delete them.

The old Fleet path cannot yet be removed: canonical archive inputs still abort, and the live legacy request/runtime/test callers below still reach it. C30/W-C32 at design `92659e25` supplies the versioned committed registry snapshot; it does not itself implement central's archive projection, receivers, finalization, notices or retained harness replacements.

## Exact cutover points

- `src/core_steps.rs:283`: `step_fleet` calls legacy `fleet_rules::call`, or bootstrap/prune/outbox on alarms and acknowledgement. Remove this entry only with its callers.
- `src/core_kind.rs:88`: `CoreKind for FleetKind` dispatches to that step. A refusal stub would weaken contracts; it is not a port.
- `src/fleet_request_admission.rs`: legacy snapshot -> shared pure fleet decision -> legacy writes/feed. Delete after equivalent canonical decisions and receipts are covered.
- `src/fleet_rules.rs`: the actual request roster below.
- `src/fleet_scope_snapshot.rs:115`: canonical `BoardProjects`, `WorkspaceArchiveInputs`, `ArchiveReports` explicitly abort. `src/fleet_snapshot.rs:103-161` supplies the old read sets and then derives known-Unheld and active reservations. Unknown lease metadata must stay unknown, never an empty/unheld default.
- `src/archive_receiver.rs`: complete the board and possibly workspace in the same transaction as the triggering accepted archive/report/refused settlement/Unheld update, preserve original actor and origin, no synthetic report or new request decision.
- `src/claim_reservation.rs` + `src/lease_admission.rs`: first settlement/tombstones and versioned lease admission; a grant must settle its own attempt with the lease update, atomically.
- Existing generic callers remain in `collaboration-central-local/src/native_effects.rs:199`, `session_runtime.rs`, `object_actors.rs`, and the retained Worker `effect_driver.rs:173`, `legacy_object_shell.rs`, `conformance_object_control.rs:70`. They require typed caller/harness migration with the cutover; deleting only the macro leaves live callers unsatisfied. Modern Tokio/Worker `ObjectRuntime<FleetKind>` is already separate.

## Request and event roster

| Legacy request/event | Equivalent owner/path and remaining work | C30 relation |
|---|---|---|
| Fleet metadata request: WorkspaceArchive / BoardArchive | Complete canonical archive snapshot, pure admission, writes/receipts/feed, immediate completion check, and owed notices. | Needs known lease/reservation projection; blocked for integrated proof. |
| Other metadata operations | Existing async `DecideFleet` + `fleet_scope_apply`; normal Fleet session route added at this anchor. Cover every old actor/origin/replay operation before deletion, including write-once creation decision bindings. | Not intrinsically blocked by C30. |
| ReserveClaim | Canonical per-attempt reservation and parent board gate, retaining a settled-first tombstone and exact attempt identity. | Reservation primitive can port independently; archive/claim race proof needs projection. |
| ReleaseClaim / refused claim settlement | Settle Refused once, never release another attempt, then run finalization in that same Fleet transaction. | Reportless finalization needs Known(Unheld) plus no active reservation. |
| DirectoryUpdate::Lease | Version admission with Fleet incarnation, content digest and mutation-time freshness; reconstruct source exclusively from public C30 snapshot. | Directly waits for C30 pin. |
| DirectoryUpdate::ClaimGranted | Admit lease side and settle that exact claim in the same Fleet transaction. Keep settlement separately receipted even if lease snapshots coalesce. | C30 for lease side; claim settlement is not discharged by repair. |
| ArchiveSeen | Parse board/project/position, bind reporter installation from session; first evidence wins; typed refusal/replay; atomic board/workspace finalization. | Final predicate needs canonical projection; pure refusal/evidence checks can be prepared separately. |
| AdvanceLeaseIncarnation | Fleet-issued witness, checked max(fence, offered)+1, persisted Registry run gating. Keep existing restore qualification. | Central-owned; not a library mutation counter. Integrated publication waits for C30. |
| DirectoryUpdate::Summary | Modern `CopyDirectorySummary` -> canonical stored copy summary already has component proof. Preserve independent sides and combine with lease projection at reads. | Copy side independent; combined lease view waits. |
| Read Directory | Canonical identities, query validation, partial/unknown sides, copy freshness and versioned lease freshness. | Lease side waits for C30. |
| Import Fleet | Modern owner/edge `import_call` -> canonical import exists; D uses that committed surface. Keep that working codec while removing the synchronous engine, and preserve import conflict/replay/time/provenance. | Import record path independent; imported-Unheld lease publication waits. D owns admin completion. |
| StoreTail / Read TailReports | Canonical paged header/membership, authorization, conflict/replay/retention/notices. Current legacy whole-row tail implementation is not the new paged contract. | A carrier/cutoff dependency; C28 lineage separate, not C30. |
| Read FleetRecords | Canonical feed snapshot/cursor/retention/independent as-of and last head after pruning. | Reader/retention independent; archive emitted records depend on archive port. |
| Fleet alarm / delivery result / lost delivery | Canonical feed pruning and debt retry on the shared alarm owner; exact acknowledgement identity; no lost debt, no effects on rollback. | General hints independent; archive holder recipients need current lease projection. |

## C30-dependent scenario families to preserve

The primary archive matrix is `fleet_archive` (5), `archive_report_storage` (4), and `claim_archive_repair` (3): 12 identities. Not every individual assertion is intrinsically dependent on C30 (for example settled-first reservation refusal), but their complete existing fixtures/call paths depend on the archive receiver and projection. Port all 12 rather than dropping helper-owned behavior.

The lease-side receiver/publication suites below add `fleet_lease_order` (6), `registry_lease_publication` (7) and `lease_order` (3, including one pure independent tuple/codec case). Directory partial-side/freshness and archive feed/notice cases are additional consumers. These groups overlap obligations; their counts are not added as independent proof of the same behavior.

The changed governing contract must be explicit for `registry_lease_publication::renewal_counter_attempt_and_publication_debt_commit_or_roll_back_together`: the library now owns revision atomicity (C30), while central's unconditional repair survives a crash with no dirty marker. Keep rollback/replay/lost-window evidence through that model; do not keep an invented second counter or silently waive the test.

C30 also creates composed-system cases the old names alone do not establish:

- Same snapshot revision read at different clocks yields the same update content, digest and `as_of`; display expiry never changes the stored digest.
- A same-time fresh renewal, an expired renewal and a shorter-expiry renewal advance the library revision and cannot rewind Fleet; a replay does not advance it.
- A crash after a library mutation with no central dirty marker still converges through public repair enumeration, including Unheld and Retired databases.
- A newer coalesced lease snapshot cannot erase the separately receipted settlement of an older claim attempt.

These need real composed public-library/Fleet evidence after the C30 pin, even if the corresponding library-only rows are already green.

## Existing cross-boundary scenarios

These remain old-harness contracts to adapt, not fulfilled by library unit tests or the new single Fleet request case:

- `fleet_request_scenarios.rs`: `run_fleet_request_conformance` and `run_fleet_conflict_conformance` (normal requests, records and conflict/replay).
- `archive_runtime.rs:112`: `run_archive_report_conformance` (normal report, reporter identity, exact board/workspace finalization and replay).
- `archive_cleanup_restart.rs:30,139`: `ArchiveCleanupRestartScenario::prepare/verify`, both `CleanupLoss::Dispatch` and `Acknowledgement` (restart repairs release debt; no synthetic report; once-only finalization).
- `directory_restart.rs:19,111`: `ImportDirectoryRestartScenario` (lost update, physical restart and independent partial copy/lease sides).
- `fleet_outbox_restart.rs:55,173`: `FleetOutboxRestartScenario`, its loss variants (persisted hint debt and real delivery acknowledgement).
- `fleet_feed_restart.rs:70,181,274`: `FleetFeedRestartScenario` / pruning continuation (persisted head, bounded feed and thirty-day retention).

## Candidate identity inventory

Each identity below is extracted from an existing permanent test declaration at this source anchor. Group notes describe the inspected owning paths; this roster is not a completed per-case removal disposition. No case is removed or ignored now. A replacement must keep its actual assertion at a real canonical store/frame boundary, or an explicit accepted retirement must name the dead implementation contract.

### fleet_archive (5)

Archive requests, reservations, reportless completion and grant/finalization race; needs canonical archive inputs and C30 integration.

- `crates/collaboration-central-core/tests/fleet_archive.rs:13` — `released_attempts_never_reactivate_or_release_a_different_attempt`
- `crates/collaboration-central-core/tests/fleet_archive.rs:47` — `releasing_the_last_reservation_finishes_archive_without_a_synthetic_report`
- `crates/collaboration-central-core/tests/fleet_archive.rs:100` — `archive_before_claim_returns_archived_without_fencing_or_granting`
- `crates/collaboration-central-core/tests/fleet_archive.rs:180` — `a_grant_at_the_unheld_timestamp_cannot_leave_archive_eligible_metadata`
- `crates/collaboration-central-core/tests/fleet_archive.rs:275` — `archive_reports_finalize_with_original_actors_without_a_request_ledger_entry`

### archive_report_storage (4)

First-report evidence, refusal, atomic finalization rollback and corrupt stored evidence; archive reader/finalizer port.

- `crates/collaboration-central-core/tests/archive_report_storage.rs:44` — `first_report_retains_all_evidence_across_successor_resends_and_completion`
- `crates/collaboration-central-core/tests/archive_report_storage.rs:95` — `typed_report_refusals_keep_records_reports_feed_and_ledger_unchanged`
- `crates/collaboration-central-core/tests/archive_report_storage.rs:165` — `finalization_failure_rolls_back_report_board_workspace_and_feed_together`
- `crates/collaboration-central-core/tests/archive_report_storage.rs:214` — `corrupt_report_metadata_is_rejected_without_silently_counting_a_report`

### claim_archive_repair (3)

Reservation before archive, a committed fence, lost cleanup dispatch and lost acknowledgement; claim settlement plus C30 projection.

- `crates/collaboration-central-core/tests/claim_archive_repair.rs:197` — `reservation_committed_before_archive_survives_lost_answer_and_registry_restart`
- `crates/collaboration-central-core/tests/claim_archive_repair.rs:236` — `fencing_is_not_abandoned_on_timeout_and_a_committed_fence_replays_after_restart`
- `crates/collaboration-central-core/tests/claim_archive_repair.rs:292` — `refused_cleanup_repairs_dropped_release_and_lost_ack_across_restarts_without_synthetic_report`

### fleet_lease_order (6)

Fleet receiver version admission, incarnation witness, coalesced updates versus separately receipted claim settlement; C30-backed projection.

- `crates/collaboration-central-core/tests/fleet_lease_order.rs:163` — `fenced_claim_repair_keeps_exact_settlement_identity_across_new_mutation_and_lost_ack`
- `crates/collaboration-central-core/tests/fleet_lease_order.rs:282` — `delayed_unseen_incarnation_eight_is_fenced_after_another_project_advanced_the_witness`
- `crates/collaboration-central-core/tests/fleet_lease_order.rs:313` — `tuple_admission_keeps_the_whole_projection_and_as_of_does_not_order_it`
- `crates/collaboration-central-core/tests/fleet_lease_order.rs:366` — `fenced_claim_grant_applies_nothing_and_newer_lease_does_not_settle_another_attempt`
- `crates/collaboration-central-core/tests/fleet_lease_order.rs:440` — `settlement_storage_failure_rolls_back_the_lease_projection_in_the_same_transaction`
- `crates/collaboration-central-core/tests/fleet_lease_order.rs:483` — `invalid_incoming_digest_and_corrupt_stored_versions_fail_without_coercion`

### registry_lease_publication (7)

Current-snapshot publication, replay, restore and stale-answer handling; C30 replaces old private lease/change reads. The old dirty-marker atomicity assertion needs explicit revised mapping to C30 revision atomicity and unconditional repair, not silent deletion.

- `crates/collaboration-central-core/tests/registry_lease_publication.rs:11` — `renewal_counter_attempt_and_publication_debt_commit_or_roll_back_together`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:58` — `lost_allocation_answer_and_crash_before_installation_keep_the_run_gated`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:101` — `registry_restored_to_three_gets_ten_from_fleets_retained_nine`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:160` — `reordering_changes_and_replaying_attempts_never_allocate_or_republish`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:217` — `obsolete_fenced_conflict_and_success_answers_cannot_clear_or_restart_newer_work`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:274` — `fenced_and_conflicting_current_offers_rebuild_from_current_rows`
- `crates/collaboration-central-core/tests/registry_lease_publication.rs:324` — `unavailable_fleet_leaves_publication_gated_but_renewal_and_replay_authoritative`

### lease_order (3)

Fleet incarnation allocator and restart gating remain central-owned. The pure version tuple/decoder test is independent of SqlTx and can be kept as such; it is not blocked by a database port.

- `crates/collaboration-central-core/tests/lease_order.rs:35` — `fleet_incarnation_fence_advances_above_both_durable_and_offered_floors`
- `crates/collaboration-central-core/tests/lease_order.rs:55` — `lease_versions_order_epoch_then_incarnation_then_change_and_reject_zero_components`
- `crates/collaboration-central-core/tests/lease_order.rs:87` — `a_restarted_registry_decides_renewal_but_gates_publication_until_fleet_allocation`

### fleet_notice_outbox (4)

Archive-holder recipients need the lease projection. Other feed-hint debt/rollback cases need canonical notice/outbox adaptation, independently of C30.

- `crates/collaboration-central-core/tests/fleet_notice_outbox.rs:9` — `fleet_mutation_only_owes_a_hint_then_alarm_retries_and_correlates_real_delivery_ack`
- `crates/collaboration-central-core/tests/fleet_notice_outbox.rs:158` — `a_later_alarm_drains_more_readers_than_one_shell_id_batch_without_losing_debt`
- `crates/collaboration-central-core/tests/fleet_notice_outbox.rs:204` — `workspace_archive_owes_one_board_hint_to_each_known_holder`
- `crates/collaboration-central-core/tests/fleet_notice_outbox.rs:264` — `a_late_outbox_failure_rolls_back_records_feed_ledger_and_high_water_together`

### fleet_feed (5)

The workspace-archive contiguous-position case depends on archive inputs. Retention, cursor and corrupt-position cases need the canonical feed reader/alarm, independently of C30.

- `crates/collaboration-central-core/tests/fleet_feed.rs:183` — `arbitrary_import_and_workspace_archive_have_contiguous_positions_without_id_stock`
- `crates/collaboration-central-core/tests/fleet_feed.rs:217` — `old_import_times_do_not_expire_feed_before_thirty_days_of_central_retention`
- `crates/collaboration-central-core/tests/fleet_feed.rs:244` — `fleet_cursor_kind_and_future_position_are_refused_without_rewinding`
- `crates/collaboration-central-core/tests/fleet_feed.rs:263` — `retention_alarm_prunes_every_entry_without_resetting_the_persisted_head`
- `crates/collaboration-central-core/tests/fleet_feed.rs:312` — `inconsistent_feed_record_position_is_rejected_without_advancing_the_reader`

### fleet_directory (5)

Lease-side freshness, Unheld and independent copy/lease ordering need C30. Invalid directory-query decoding itself is independent but its legacy receiver still needs replacement.

- `crates/collaboration-central-core/tests/fleet_directory.rs:84` — `real_fleet_keeps_summary_by_position_and_lease_by_independent_registry_version`
- `crates/collaboration-central-core/tests/fleet_directory.rs:106` — `unheld_directory_update_changes_only_lease_and_never_creates_fleet_records`
- `crates/collaboration-central-core/tests/fleet_directory.rs:269` — `public_directory_keeps_partial_sides_visible_and_filters_only_known_metadata`
- `crates/collaboration-central-core/tests/fleet_directory.rs:371` — `public_directory_exposes_independent_freshness_without_late_or_equal_rewinds`
- `crates/collaboration-central-core/tests/fleet_directory.rs:428` — `public_directory_refuses_an_invalid_typed_query`

### fleet_requests (5)

Canonical request actor/envelope/replay cases. Normal async route now has one real whole-server-reopen scenario; that is not a one-for-one disposition for every old case.

- `crates/collaboration-central-core/tests/fleet_requests.rs:126` — `different_declared_humans_are_admitted_over_the_same_verified_carrier`
- `crates/collaboration-central-core/tests/fleet_requests.rs:154` — `fleet_request_id_must_match_its_carrier_envelope_before_admission`
- `crates/collaboration-central-core/tests/fleet_requests.rs:205` — `a_declared_foreign_session_is_preserved_separately_from_the_verified_carrier`
- `crates/collaboration-central-core/tests/fleet_requests.rs:260` — `router_actor_origin_must_match_the_verified_fleet_carrier`
- `crates/collaboration-central-core/tests/fleet_requests.rs:269` — `decided_replay_and_conflicting_request_preserve_every_fleet_row_and_feed_entry`

### fleet_origin (4)

Admitted origin and relabel/registration barriers. New route reads saved admission origin; remaining distinct old scenarios still need permanent real-path coverage.

- `crates/collaboration-central-core/tests/fleet_origin.rs:110` — `human_fleet_records_use_the_enrolled_registration_origin`
- `crates/collaboration-central-core/tests/fleet_origin.rs:125` — `a_frame_supplied_machine_label_cannot_change_fleet_state`
- `crates/collaboration-central-core/tests/fleet_origin.rs:143` — `a_changed_enrollment_label_is_adopted_only_at_registration_and_never_rewrites_old_records`
- `crates/collaboration-central-core/tests/fleet_origin.rs:186` — `fleet_frames_wait_for_registry_registration_instead_of_inventing_an_origin`

### import_claim (7)

Coupled legacy import/claim/floor/summary cases: mechanisms belong to public library workflows; actual central gates, publication/reopen and directory interaction still need composed-core proof. D owns admin completion.

- `crates/collaboration-central-core/tests/import_claim.rs:166` — `staged_import_is_invisible_and_reimport_never_changes_the_summary`
- `crates/collaboration-central-core/tests/import_claim.rs:196` — `claim_fences_before_grant_and_one_attempt_keeps_one_epoch_and_expiry`
- `crates/collaboration-central-core/tests/import_claim.rs:269` — `copy_fence_restore_guard_refuses_claim_without_grant_or_floor_repair`
- `crates/collaboration-central-core/tests/import_claim.rs:294` — `late_directory_ack_cannot_remove_a_newer_summary_outbox_row`
- `crates/collaboration-central-core/tests/import_claim.rs:383` — `stored_import_metadata_for_another_project_is_rejected_explicitly`
- `crates/collaboration-central-core/tests/import_claim.rs:412` — `rerunning_import_repairs_the_copy_to_registry_publication_gap`
- `crates/collaboration-central-core/tests/import_claim.rs:445` — `claimed_copy_preserves_holder_change_floor_repair_semantics`

### rejected_tail (7)

Tail verification/storage/conflict/notice/replacement cases; depends on A carrier and canonical paged tail storage, not C30 itself. C28 adds lineage separately.

- `crates/collaboration-central-core/tests/rejected_tail.rs:120` — `historical_holder_is_verified_before_its_tail_reaches_the_real_fleet`
- `crates/collaboration-central-core/tests/rejected_tail.rs:137` — `never_writer_is_refused_without_a_fleet_store_or_tail_notice`
- `crates/collaboration-central-core/tests/rejected_tail.rs:195` — `first_tail_wins_with_exact_conflict_and_no_duplicate_hint_or_request_ledger`
- `crates/collaboration-central-core/tests/rejected_tail.rs:262` — `full_tail_read_is_project_scoped_and_orders_numeric_epochs`
- `crates/collaboration-central-core/tests/rejected_tail.rs:295` — `holder_change_after_verification_cannot_strand_the_durable_report`
- `crates/collaboration-central-core/tests/rejected_tail.rs:357` — `replaced_or_expired_session_never_stores_a_late_verification_result`
- `crates/collaboration-central-core/tests/rejected_tail.rs:408` — `a_late_hint_failure_rolls_back_tail_and_corrupt_stored_evidence_is_refused`

Inventory total: **65 candidate identities in 13 files**. This is not a claim that all 65 are blocked by C30 or that this roster contains every legacy central test; additional shared harness callers remain in scope at the actual removal gate.

## Execution boundary

No new mechanism is started. The normal Fleet route is separately committed/qualified. The cutoff implementation/test is preserved as an uncompiled, unregistered exact patch pending A's carrier; C28 lineage is held. The archive/lease path waits for the declared C30 pin and public API. Once those dependencies arrive: re-anchor the public APIs, port the above canonical reads/receivers and tests, observe wrong-build sensitivity and required native/Worker behavior, then delete the legacy Fleet path and its obsolete harness arms together. A compiler stub, test deletion or an unobserved stand-in does not close that gate.
