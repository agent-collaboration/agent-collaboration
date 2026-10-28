use super::{ItemEdge, ModuleGraph, parse_use_edges};

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn parser_retains_only_uses_and_deduplicates_edges() -> Result<(), crate::ArchitectureError> {
    let edges = parse_use_edges(
        r##"digraph {
    "demo::a::First" -> "demo::b::Second" [label="uses", color="#fff"];
    "demo::a::First" -> "demo::b::Second" [label="uses"];
    "demo::a" -> "demo::a::First" [label="owns"];
    "demo::b::Second" -> "demo::a::First" [ label = "uses" ];
}"##,
    )?;
    assert_eq!(
        edges,
        vec![
            ItemEdge {
                from: "demo::a::First".to_owned(),
                to: "demo::b::Second".to_owned(),
            },
            ItemEdge {
                from: "demo::b::Second".to_owned(),
                to: "demo::a::First".to_owned(),
            },
        ]
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn parser_decodes_quoted_endpoints() -> Result<(), crate::ArchitectureError> {
    let edges = parse_use_edges(
        r#"digraph {
    "demo::a::say\"hello" -> "demo::b::path\\name" [label="uses"];
}"#,
    )?;
    assert_eq!(
        edges,
        vec![ItemEdge {
            from: "demo::a::say\"hello".to_owned(),
            to: "demo::b::path\\name".to_owned(),
        }]
    );
    Ok(())
}

#[test]
fn parser_rejects_broken_or_incomplete_tool_output() {
    for dot in [
        "",
        "digraph {",
        "digraph {\n broken [label=\"uses\"];\n}",
        "digraph {\n \"demo::a\" -> missing [label=\"uses\"];\n}",
        "digraph {\n \"demo::a\" -> \"demo::b\";\n}",
    ] {
        assert!(parse_use_edges(dot).is_err(), "accepted {dot}");
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn fold_discards_root_internal_and_external_edges() -> Result<(), crate::ArchitectureError> {
    let graph = ModuleGraph::from_dot(
        r#"digraph {
    "demo" -> "demo::a::First" [label="uses"];
    "demo::a::First" -> "demo" [label="uses"];
    "demo::a::First" -> "demo::a::nested::Second" [label="uses"];
    "demo::a::First" -> "other::b::Second" [label="uses"];
    "demo::a::First" -> "demo::b::Second" [label="uses"];
    "demo::b::Second" -> "demo::a::nested::Third" [label="uses"];
}"#,
        "demo",
    )?;
    assert_eq!(
        graph.module_edges,
        std::collections::BTreeMap::from([
            (
                "demo::a".to_owned(),
                std::collections::BTreeSet::from(["demo::b".to_owned()])
            ),
            (
                "demo::b".to_owned(),
                std::collections::BTreeSet::from(["demo::a".to_owned()])
            ),
        ])
    );
    assert_eq!(graph.cycle_components(), vec![vec!["demo::a", "demo::b"]]);
    assert_eq!(
        graph.cycle_item_edges(&["demo::a".to_owned(), "demo::b".to_owned()]),
        vec![
            ItemEdge {
                from: "demo::a::First".to_owned(),
                to: "demo::b::Second".to_owned(),
            },
            ItemEdge {
                from: "demo::b::Second".to_owned(),
                to: "demo::a::nested::Third".to_owned(),
            },
        ]
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn sccs_separate_cycles_from_one_way_bridges() -> Result<(), crate::ArchitectureError> {
    let graph = ModuleGraph::from_dot(
        r#"digraph {
    "demo::a" -> "demo::b" [label="uses"];
    "demo::b" -> "demo::a" [label="uses"];
    "demo::b" -> "demo::c" [label="uses"];
    "demo::c" -> "demo::d" [label="uses"];
    "demo::d" -> "demo::c" [label="uses"];
    "demo::d" -> "demo::e" [label="uses"];
}"#,
        "demo",
    )?;
    assert_eq!(
        graph.cycle_components(),
        vec![vec!["demo::a", "demo::b"], vec!["demo::c", "demo::d"]]
    );
    Ok(())
}
