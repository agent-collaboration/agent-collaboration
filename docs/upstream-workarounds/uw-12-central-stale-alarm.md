# UW-12: replace stale central alarms at event entry

Status: in place. Dependency: runtime-primitives 0.4.0, pinned through
agent-fleet-sync `84935efdab03615843611bb5723983d7655e6e26`.

`AlarmOwner::wake_at` preserves any stored deadline at or before a requested
deadline, even when the stored deadline is already in the past. The shared
DurableObjectStore requires that concrete owner for transactional wakes, so
central repairs a stale alarm before entering its runtime rather than replacing
the library owner or changing the pin.

`ShellActor::rearm_stale_alarm` reads and replaces a past alarm with the current
instant inside the platform concurrency gate. Existing present/future deadlines
keep the library's arbitration. Storage errors are returned as values, matching
the existing local-section wrapper; they do not throw the gate callback and
reset the object.

Recreate through the permanent `session_scope_workerd` scenario `stale-alarm`.
Its fault-only fixture seeds a real Fleet platform alarm at `1900000010000`,
injects time `1900000015000`, requests both a transactional and direct wake at
`1900000020000`, and requires the actual alarm to become `1900000015000`.
The dates are in the wall clock's future, so no timer delay repairs the test.
The original D1 Worker retains `1900000010000` and fails this check.

Remove this event-entry workaround only after a pinned upstream owner handles
past alarms on transactional and direct wake paths, and the stale case passes
without it. Preserve the valid future-deadline arbitration check too.

Fleet's separate error isolation is permanent retention behavior: prune each
machine in its own transaction, continue other machines and due jobs, and rearm
the journal after errors. The two-machine hole regression must continue to
return a visible corruption error while proving healthy pruning/job progress.
Neither behavior changes a schema, journal identity, wire protocol or library.
