# Router update and board import tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p agent-router-update
```

`integration/main.rs` gathers 26 former external roots covering the `board-import`
subcommand: legacy transforms, identities, attribution and refusals, source and
seed integrity, message cutover, native inbox fidelity, publication and fleet
owner routes. Its `update_ledger_journey` root drives the real update binary
(J0) on a copy of `fixtures/populated-root`: refusal while the Host holds the
root, a dry run that changes no byte, kills at each step-0001 barrier followed
by a rerun, a never-connected root, an unexplained `fleet.sqlite` and a Host
stopped before recording its new store. Its `update_moves_journey` root (J4)
covers steps 0005 and 0010: every fixture value carried into `fleet.sqlite`, a
kill at each merge and rename barrier followed by a rerun, merged rows still
only in the log right before the rename checkpoint, and a malformed source that
stops the merge before it changes anything. Its `update_source_checks` root (J4)
covers what step 0005 refuses and what it never reads: a well-formed older store
holding a value its owning decoder refuses (cursor key, outbox request, creation
input), a cursor key other than the one the Fleet database holds, and an older
store that reappears after its rows moved, removed unread. Its
`update_reset_and_dead_files` root (J4) covers steps 0002 and 0009: each phase of
an older enrollment reset finished against the store under its former name
before any store moves, a pending reset on a root with no Fleet store, an
unreadable journal, approval routes imported before
the dead files are removed, the root listing afterwards, malformed routes, and
the Host guard on a dead file, including one back after the update and one on a
never-connected root. Its `update_whole_journey` root (J0 and J4 together) runs
every PR A step in order on one root holding everything an older Router leaves:
the populated stores, old settings, the old proxy store with rows only in its
log, dead files with approval routes, an unfinished reset, and the identity and
enrollment files PR B moves, which keep their bytes. A full update, and
a kill at every barrier of every step followed by a rerun, end at the same root:
each step's values through their owners and an exact listing of the root. These
cases sign the binary with
the debug runner before launching it, and read values raw from the stores, so
their oracle is independent of the tool's code.
`e2e/main.rs` retains the former `upgrade_binary_e2e` root, which drives the real
importer CLI through preparation, publication, replay and restart journeys.
Library and binary unit tests remain colocated.

Run only the serial CLI journeys after preparing the artifacts as CI does:

```sh
cargo nextest run --locked --profile ci -p agent-router-update --test e2e --test-threads 1
```

Each aggregate declares shared helper implementations once. The actual CLI
publication fixture lives in legacy-upgrade-test-support; package tests and
external consumers use that library through dev-dependencies. Case expansions
and fixture data stay in their existing directories.

A former binary `<root>` and test `<case>` becomes binary `integration` or
`e2e` and test `<root>::<case>`. Unit-test identities do not change. CI retains
the same main/e2e split, scheduling and deadlines; nextest uses one process per
case. The message-copy child selector derives from `module_path!()`. Its
intentional exit 93, durable copy and replay/drop checks reject zero matches.
