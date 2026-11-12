# collaboration-central-tokio test contracts

This package currently declares no integration or e2e test target. The former
suites drove Copy, replication, forwarding, mail and the deleted conformance
harness, and were removed with those features. The kept enrollment, Access and
session-upgrade behavior is proven by `collaboration-central-core`'s tests until
the workspace-storage redesign adds this host's own tests.
