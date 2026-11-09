# Clock tests

Unit tests beside the implementation cover manual deadline registration, one
advance waking multiple sleepers, cancellation and UTC/elapsed consistency.
There is no integration or whole-process target in this leaf package.

Run with `cargo nextest run --profile ci -p router-clock`. The CI profile supplies
the outer hang bound; the tests themselves never wait on a native timer.
