# Central engine model tests

This pure crate has no integration or e2e target. Sibling `*_test.rs` modules cover declared transition reachability, deterministic reducer inputs, failure retirement thresholds, and each alarm-selection branch using supplied times and UUIDv7 values. The adjacent positive and compile-fail `Machine` doctests prove state-match exhaustiveness. Host integration tests own transactional, concurrency, and physical-alarm proof.
