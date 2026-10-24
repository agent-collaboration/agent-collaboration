# Held legacy upgrade claims

The Lead's 2026-10-18 disposition (Router push
`01a150bf-87ab-7cf0-aeda-08cabd379b1d`) preserves four obligations and retires
four dead contracts under specification 1 §5 and Program Design 2 §3d. The
importer opens legacy files read-only; it never adopts or migrates them.

| Old case | Obligation or dead-contract reason | Current proof |
| --- | --- | --- |
| `fresh_schema_reopens_with_single_seed_and_foreign_keys` | Retired: in-place adoption of the legacy store is forbidden. The destination is a separate native store. | No replacement for the dead adoption contract. Native destination readback remains covered by `legacy_transform.rs`. |
| `changed_schema_is_rejected_without_replacing_data` | Keep: refuse a changed shipped schema without changing source data. | Same-named case in `source_schema_refusals.rs`: missing shipped index; source bytes and modification time unchanged. |
| `untracked_schema_is_not_adopted` | Keep: refuse a source without its migration history; never create it. | Same-named case in `source_schema_refusals.rs`: removed history table; source bytes and modification time unchanged. |
| `extra_checkpoint_row_is_rejected` | Keep: require the shipped singleton checkpoint. | Same-named case in `source_schema_refusals.rs`: extra checkpoint row; source bytes and modification time unchanged. |
| `subscription_window_schema_has_no_overflow_column` | Retired: the importer reads the shipped schema but does not own or evolve its shape. | No replacement for the dead schema-migration contract. The complete shipped schema is checked read-only. |
| `backfill_requires_open_session_unresolved_thread_and_active_watch` | Retired: the supported source has already run its migrations. An older source must be refused rather than backfilled. | `missing_or_changed_migration_identity_is_rejected_before_data_decode` in `source_schema_refusals.rs` refuses missing or altered migration history before decoding invalid board data. |
| `p1_backfill_delivered_stays_at_message_activity_when_join_is_latest` | Retired: delivery progress is not imported (specification 1 §5). | No replacement for the dead delivery-backfill contract. Bookmark cutoff fidelity is covered separately by `native_inbox_fidelity.rs`. |
| `a_stored_control_character_reads_back_escaped_instead_of_raw` | Keep: preserve exactly the escaped historical text returned by the shipped reader. | `historical_control_text_and_derived_subject_keep_the_shipped_visible_escape` in `historical_text.rs`: native Post and derived Thread readback, with unchanged source bytes. |

The three original refusal replacements and the additional migration-identity
case each failed before the schema guard and passed afterwards (4 cases). The
historical-text case first failed on the derived subject's raw control and
passed after deriving the subject from the visible escaped text (1 case).
These are file-backed shipped fixtures and native destination readback, not
source-string assertions. The old held files and their cfg gate were removed
only after all kept obligations had passing replacements. Watch coverage's two
separate native-reader regressions were kept enabled and passed unchanged after
the owning live fix at `28619181`; no importer workaround was added.
