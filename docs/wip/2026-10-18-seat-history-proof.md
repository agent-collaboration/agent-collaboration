# Seat history correction

This bounded slice implements PD2 §3d's reviewed interval rule. Each explicit
join closes its actor's open interval as `rejoined` and opens a new interval.
HandTo closes the target's previous interval as `promoted` and opens its
orchestrator interval. The outgoing holder remains closed as `replaced`.
Historical role, note and join key stay on the closed row. Existing refusal
checks run before either change, and closure precedes insertion.

The native codec and validated participant type accept the two new reasons.
The participant-list output schema adds only those values. No SQL schema or
library dependency changes belong to this slice.

Two permanent regressions use actual native project admission and the public
participant-history API. They cover advisor-to-reviewer without a leave or
post, and advisor-to-orchestrator handover without an intervening post. Both
check historical roles, note preservation, join/closure positions, current
holder state, replay idempotency and owning native reopen. A new distinct
same-role join also preserves its predecessor and accepts a replacement note.

| Gate | Result |
|---|---|
| Original native/API regressions, before the fix | 0 passed / 2 failed, exit 101: each retained one interval instead of two |
| Corrected regressions | 2 / 0, exit 0 |
| Old in-place plan restored as a wrong-build control | Compiled, 0 / 2, exit 101 at the same history assertions |
| Corrected bytes restored, same regressions | 2 / 0, exit 0 |
| Participant-related service cases | 14 / 0, exit 0 |
| Participant constructor round-trip and invalid-state case | 1 / 0, exit 0 |
| Full current catalog golden comparison | 1 / 0, exit 0; apparatus qualification below |
| Domain, storage and native-regression Clippy, warnings denied | All exit 0 |
| Formatting and diff checks | Exit 0 |

The corrected plan's restored SHA256 was
`ecafbb447fbdbd0620a8d7f73e121cad5191694d4d37260ad5cdb31f1ef41359`.
The wrong build changed the plan only; it did not weaken the tests or codec.
These controls supplement the genuine pre-fix red.

The retained case
`create_and_repeat_join_preserve_one_valid_participant_and_explicit_watch`
is renamed to
`create_and_repeated_joins_retain_history_and_one_open_participant`.
Its watch, note, leave and later-rejoin assertions remain; its history counts
now require every interval and exactly one open participant.

This early dependency commit precedes the A3a fixture cutover. The 14-case group
used that working tree's normalized fault fixtures, and the catalog test
compiled with its native MCP fixture adaptations. Those apparatus repairs stay
in the following cutover commit. The catalog refresh exposed 97 older A2
schema entries needing that cutover; only the participant-list reason enum is
included here. No full workspace test or compile result is claimed for this
early slice. The next declared target must include the cutover and its required
gates. The work trace is kept outside the public tree.
