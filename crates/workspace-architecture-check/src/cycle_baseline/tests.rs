#![expect(
    clippy::panic_in_result_fn,
    reason = "test assertions report contract failures; Result propagates setup errors"
)]

use super::{CycleBaseline, PackageGraphs};
use crate::ModuleGraph;

const TWO_MODULE_BASELINE: &str = r#"{"demo-package":[{
    "modules":["demo::a","demo::b"],
    "edges":[{"from":"demo::a","to":"demo::b"},{"from":"demo::b","to":"demo::a"}]
}]}"#;

const THREE_MODULE_BASELINE: &str = r#"{"demo-package":[{
    "modules":["demo::a","demo::b","demo::c"],
    "edges":[{"from":"demo::a","to":"demo::b"},{"from":"demo::b","to":"demo::c"},{"from":"demo::c","to":"demo::a"}]
}]}"#;

fn packages(dot: &str) -> Result<PackageGraphs, crate::ArchitectureError> {
    Ok(PackageGraphs::from([(
        "demo-package".to_owned(),
        vec![ModuleGraph::from_dot(dot, "demo")?],
    )]))
}

fn two_module_cycle() -> Result<PackageGraphs, crate::ArchitectureError> {
    packages(
        r#"digraph {
    "demo::a::First" -> "demo::b::Second" [label="uses"];
    "demo::b::Second" -> "demo::a::First" [label="uses"];
}"#,
    )
}

#[test]
fn unchanged_cycles_pass_independent_of_baseline_order() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(
        r#"{"demo-package":[{
        "modules":["demo::b","demo::a"],
        "edges":[{"from":"demo::b","to":"demo::a"},{"from":"demo::a","to":"demo::b"}]
    }]}"#,
    )?;
    assert!(baseline.compare(&two_module_cycle()?).is_ok());
    assert_eq!(baseline, CycleBaseline::from_json(TWO_MODULE_BASELINE)?);
    Ok(())
}

#[test]
fn new_cycle_fails_with_its_concrete_edges() -> Result<(), crate::ArchitectureError> {
    let violation = CycleBaseline::default()
        .compare(&two_module_cycle()?)
        .expect_err("a new SCC must fail");
    assert!(violation.has_growth());
    assert_eq!(
        violation.to_string(),
        "demo-package: new or enlarged SCC: demo::a, demo::b\n  demo::a::First -> demo::b::Second\n  demo::b::Second -> demo::a::First\n"
    );
    Ok(())
}

#[test]
fn module_joining_an_allowed_cycle_fails() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(TWO_MODULE_BASELINE)?;
    let current = packages(
        r#"digraph {
    "demo::a" -> "demo::b" [label="uses"];
    "demo::b" -> "demo::a" [label="uses"];
    "demo::b" -> "demo::c" [label="uses"];
    "demo::c" -> "demo::a" [label="uses"];
}"#,
    )?;
    let violation = baseline
        .compare(&current)
        .expect_err("joining an SCC must fail");
    assert!(violation.has_growth());
    assert!(violation.to_string().contains("demo::a, demo::b, demo::c"));
    assert!(violation.to_string().contains("demo::c -> demo::a"));
    assert!(!violation.to_string().contains("shrank"));
    Ok(())
}

#[test]
fn new_intra_scc_edge_fails_without_membership_growth() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(THREE_MODULE_BASELINE)?;
    let current = packages(
        r#"digraph {
    "demo::a" -> "demo::b" [label="uses"];
    "demo::b" -> "demo::c" [label="uses"];
    "demo::c" -> "demo::a" [label="uses"];
    "demo::a::NewUse" -> "demo::c::Target" [label="uses"];
}"#,
    )?;
    let violation = baseline
        .compare(&current)
        .expect_err("new internal edge must fail");
    assert!(violation.has_growth());
    assert_eq!(
        violation.to_string(),
        "demo-package: new module edge inside or into SCC: demo::a -> demo::c\n  demo::a::NewUse -> demo::c::Target\n"
    );
    Ok(())
}

#[test]
fn new_incoming_edge_fails_without_membership_growth() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(TWO_MODULE_BASELINE)?;
    let current = packages(
        r#"digraph {
    "demo::a" -> "demo::b" [label="uses"];
    "demo::b" -> "demo::a" [label="uses"];
    "demo::outside::NewUse" -> "demo::a::Target" [label="uses"];
}"#,
    )?;
    let violation = baseline
        .compare(&current)
        .expect_err("new incoming edge must fail");
    assert_eq!(
        violation.to_string(),
        "demo-package: new module edge inside or into SCC: demo::outside -> demo::a\n  demo::outside::NewUse -> demo::a::Target\n"
    );
    Ok(())
}

#[test]
fn new_item_use_of_an_allowed_module_edge_passes() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(TWO_MODULE_BASELINE)?;
    let current = packages(
        r#"digraph {
    "demo::a::NewUse" -> "demo::b::DifferentItem" [label="uses"];
    "demo::b::NewUse" -> "demo::a::DifferentItem" [label="uses"];
    "demo::a" -> "demo::outside" [label="uses"];
}"#,
    )?;
    assert!(baseline.compare(&current).is_ok());
    Ok(())
}

#[test]
fn cycles_cannot_merge_two_allowed_components() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(
        r#"{"demo-package":[
        {"modules":["demo::a","demo::b"],"edges":[{"from":"demo::a","to":"demo::b"},{"from":"demo::b","to":"demo::a"}]},
        {"modules":["demo::c","demo::d"],"edges":[{"from":"demo::c","to":"demo::d"},{"from":"demo::d","to":"demo::c"}]}
    ]}"#,
    )?;
    let current = packages(
        r#"digraph {
    "demo::a" -> "demo::b" [label="uses"];
    "demo::b" -> "demo::c" [label="uses"];
    "demo::c" -> "demo::d" [label="uses"];
    "demo::d" -> "demo::a" [label="uses"];
}"#,
    )?;
    assert!(
        baseline
            .compare(&current)
            .expect_err("merged SCC must fail")
            .has_growth()
    );
    Ok(())
}

#[test]
fn shrink_and_disappearance_require_a_baseline_update() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(
        r#"{"demo-package":[{
        "modules":["demo::a","demo::b","demo::c"],
        "edges":[{"from":"demo::a","to":"demo::b"},{"from":"demo::b","to":"demo::a"}]
    }]}"#,
    )?;
    for current in [two_module_cycle()?, PackageGraphs::new()] {
        let violation = baseline
            .compare(&current)
            .expect_err("stale baseline must fail");
        assert!(!violation.has_growth());
        assert!(
            violation
                .to_string()
                .contains("update the baseline (it may only shrink)")
        );
    }
    Ok(())
}

#[test]
fn removing_an_incoming_edge_requires_a_baseline_update() -> Result<(), crate::ArchitectureError> {
    let baseline = CycleBaseline::from_json(
        r#"{"demo-package":[{
        "modules":["demo::a","demo::b"],
        "edges":[{"from":"demo::a","to":"demo::b"},{"from":"demo::b","to":"demo::a"},{"from":"demo::outside","to":"demo::a"}]
    }]}"#,
    )?;
    let violation = baseline
        .compare(&two_module_cycle()?)
        .expect_err("removed edge must ratchet");
    assert!(!violation.has_growth());
    assert!(
        violation
            .to_string()
            .contains("update the baseline (it may only shrink)")
    );
    Ok(())
}

#[test]
fn acyclic_new_package_passes() {
    let current = PackageGraphs::from([(
        "new-package".to_owned(),
        vec![ModuleGraph::fold_edges(Vec::new(), "new_package")],
    )]);
    assert!(CycleBaseline::default().compare(&current).is_ok());
}

#[test]
fn malformed_components_and_edges_are_rejected() {
    for json in [
        r#"{"p":[{"modules":[],"edges":[]}]}"#,
        r#"{"p":[{"modules":["a"],"edges":[]}]}"#,
        r#"{"p":[{"modules":["a","a"],"edges":[]}]}"#,
        r#"{"p":[{"modules":["a","b"],"edges":[]},{"modules":["b","c"],"edges":[]}]}"#,
        r#"{"p":[{"modules":["a","b"],"edges":[{"from":"a","to":"outside"}]}]}"#,
        r#"{"p":[{"modules":["a","b"],"edges":[{"from":"a","to":"a"}]}]}"#,
        r#"{"p":[{"modules":["a","b"],"edges":[{"from":"a","to":"b"},{"from":"a","to":"b"}]}]}"#,
    ] {
        assert!(CycleBaseline::from_json(json).is_err(), "accepted {json}");
    }
}
