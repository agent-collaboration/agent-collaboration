# In-flight mail upgrade seed

`old-producer.sql` is a SQLite dump of the closed machine main+WAL bundle from
`o8-maintained-old-producer-05/old-runtime-state/a/interaction.sqlite` in the
D1 Verifier's retained `tmp/d1-verify/apparatus-groups/` evidence.
Producer: Router `cbf56bad640ae981900c646ff2ae0b64aaaebd3b`, library `b9da4b5`.
The source bundle is copied before reading; the original evidence is untouched.
The dump retains the producer's migration checksums, pending submit, completed
gate, call intent, sender context, item ID and payload.

The handed-off test derives an accepted snapshot using the old library's
`outbox_rules.rs::mark_submitted`: Sent/version 1, released payload, completed
submit. This accepted variant is a test arrangement, not a second captured run.
The remote answer in the pending test stands in for central; the real opener,
journal engine, submit task, sender binding and public status read stay real.
