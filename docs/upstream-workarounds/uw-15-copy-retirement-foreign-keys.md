# UW-15: defer foreign keys during Copy retirement

Status: in place. Dependency: agent-fleet-sync receiver, pinned to
`b75d072c116afa46cfa82facd34db6317f380687` on its maintenance branch.

`retire_copy` deletes replicated application rows in table-name order. A valid
parent table can sort before a child that references it. Cloudflare's SQLite
backend enforces foreign keys, so deleting that parent first raises a foreign-key
error. The original one-table retirement fixture could not expose the defect;
the native Copy backend also opens with foreign keys disabled.

The pinned receiver reuses its existing transaction-scoped
`ForeignKeyIntegrity::defer` and `restore` around the application-table deletes.
Identity, fence and ACK checks still precede deletion. All application tables
must be empty before constraints are restored and the minimal retirement receipt
is written. No schema, migration, wire format or public generic bound changes.
The private helper's decoding bounds now sit on `validate`, the method that
actually decodes those values.

Recreate with the permanent `session_scope_workerd` scenario `owner-retirement`:
the imported-unheld fixture seeds a parent and a referencing child, and the
normal reader must observe both literal rows before retirement. At the previous
`bc5935b` pin, the actual Rust/DO dispatch raised a foreign-key exception and
remained pending until the unchanged eight-second bound. At the corrected pin,
the same journey completes and retains its terminal owner reply, directory,
Copy receipt, replay and no-recreation assertions. The existing native receiver
retirement tests remain required; they alone do not exercise Cloudflare SQL.

The staging old-corpus repair is a separate pin,
`920f9de986a2ee277496c614c9918babf53454f1`, based on the original `b9da4b5`
revision. It contains the same two-file correction while preserving the old
migration corpus. Do not replace an old staging Worker with the current pin
before the storage migration. Local reproduction identifies the failure
mechanism; hosted incident attribution still requires completing the retained
pending operation through the normal protocol.

Replace the maintenance pin only after the chosen upstream revision contains
the correction and the same referenced-data workerd journey passes. Keep that
regression: a version upgrade or native-only pass does not establish the fix.
