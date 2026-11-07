# UW-8: replace stale central alarms before entering the shared runtime

Status: in place. Dependency: `runtime-primitives` 0.4.0, pinned through
agent-fleet-sync commit `b9da4b5f230f81430fdf1140fbb634cf64decd8e`.

`AlarmOwner::wake_at` keeps any stored alarm at or before the requested deadline,
including an alarm that is already in the past. After a failed alarm callback,
that comparison can suppress every subsequent wake request. The library's
`DurableObjectStore` requires this concrete alarm owner for transactional wakes.

Central's `ShellActor::runtime` replaces a past alarm with the current instant
before initialization or runtime work can request another wake. Its platform
`block_concurrency_while` section keeps the storage read and replacement together.
An existing present or future alarm retains the shared owner's arbitration.
The dependency pin, journal identities, retry rules and wire protocol are unchanged.

The session runtime separately re-reads the journal's earliest deadline and
ensures a wake after either a successful drive or an earlier maintenance error.
The original error remains an error.

Recreate with the permanent `session-capabilities` workerd scenario: the
conformance-only fixture seeds a real alarm at `1900000010000` and logical time
at `1900000015000`. Both dates remain in the platform clock's future. The next
ordinary event must observe the alarm replaced with `1900000015000`. Before
this workaround, the scenario reads the original stale value and fails.

The existing `session_alarm_rearms_admitted_work_after_retention_failure` native
test checks that a maintenance error preserves a pending job's wake and that
the same admitted job completes after the corrupt fixture row is removed.

Remove the event-entry workaround only after a pinned upstream alarm owner
handles stale deadlines for both transactional and direct wake requests, and the
workerd stale-alarm and same-object future-deadline arbitration cases pass with
the workaround removed. Re-run the native maintenance-failure regression too.
Local conformance does not establish production recovery.
