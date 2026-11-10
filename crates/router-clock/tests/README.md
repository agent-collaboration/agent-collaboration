# Clock tests

Unit tests beside the implementation cover manual deadline registration, one
advance waking multiple sleepers, cancellation and UTC/elapsed consistency.
Timeout cases preserve ready-result precedence and start the budget when the
wait is created, even if it is first polled after the clock advances.
There is no integration or whole-process target in this leaf package.

Run with `cargo nextest run --profile ci -p router-clock`. The CI profile supplies
the outer hang bound; the tests themselves never wait on a native timer.
