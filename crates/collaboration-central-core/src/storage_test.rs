use super::RowExpectation;

#[test]
fn row_expectations_admit_only_their_counts() {
    assert!(RowExpectation::Any.admits(0));
    assert!(RowExpectation::Any.admits(7));
    assert!(RowExpectation::Exactly(1).admits(1));
    assert!(!RowExpectation::Exactly(1).admits(0));
    assert!(!RowExpectation::Exactly(1).admits(2));
    assert!(RowExpectation::AtLeastOne.admits(3));
    assert!(!RowExpectation::AtLeastOne.admits(0));
}
