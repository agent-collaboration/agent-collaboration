# State machine core tests

This pure crate has no integration or e2e target. The sibling `transition_check_test.rs` covers the shared declared-transition check: a machine whose reducer matches its table passes, an unreached declared edge and an undeclared taken edge are each reported by name. The adjacent positive and compile-fail `StateMachine` doctests prove state-match exhaustiveness. Central's engine `Machine` keeps its own tests in central-engine-model.
