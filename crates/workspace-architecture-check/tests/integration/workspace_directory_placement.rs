use super::{PolicyResult, source_path_policy::collect_rust_sources};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use syn::{Item, UseTree};

#[test]
fn core_consumes_workspace_directory_without_implementing_it() -> PolicyResult {
    let core_source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../collaboration-central-core/src");
    let mut sources = Vec::new();
    collect_rust_sources(&core_source, &core_source, &mut sources)?;
    if sources.is_empty() {
        return Err("Core source scan selected no files".into());
    }
    sources.sort();
    for source in sources {
        check_core_directory_placement(&fs::read_to_string(&source)?, &source)?;
    }
    Ok(())
}

fn check_core_directory_placement(source: &str, path: &Path) -> PolicyResult {
    let parsed = syn::parse_file(source)
        .map_err(|error| format!("{}: Rust parse failed: {error}", path.display()))?;
    if implements_directory(
        &parsed.items,
        &BTreeSet::from(["WorkspaceDirectory".to_owned()]),
    ) {
        return Err(format!(
            "{}: collaboration-central-core must consume WorkspaceDirectory through fleet-contracts; fleet-workspace-directory owns the production implementation",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn implements_directory(items: &[Item], inherited_names: &BTreeSet<String>) -> bool {
    let mut trait_names = inherited_names.clone();
    for item in items {
        if let Item::Use(import) = item {
            collect_directory_aliases(&import.tree, &mut trait_names);
        }
    }
    items.iter().any(|item| match item {
        Item::Impl(implementation) => implementation
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_some_and(|segment| trait_names.contains(&segment.ident.to_string())),
        Item::Mod(module) => module
            .content
            .as_ref()
            .is_some_and(|(_, items)| implements_directory(items, &trait_names)),
        _ => false,
    })
}

fn collect_directory_aliases(tree: &UseTree, trait_names: &mut BTreeSet<String>) {
    match tree {
        UseTree::Path(path) => collect_directory_aliases(&path.tree, trait_names),
        UseTree::Group(group) => {
            for import in &group.items {
                collect_directory_aliases(import, trait_names);
            }
        }
        UseTree::Rename(rename) if rename.ident == "WorkspaceDirectory" => {
            trait_names.insert(rename.rename.to_string());
        }
        _ => {}
    }
}

#[test]
fn workspace_directory_placement_rejects_direct_and_aliased_core_implementations() {
    let path = Path::new("collaboration-central-core/src/directory.rs");
    check_core_directory_placement(
        "fn show(directory: &impl fleet_contracts::WorkspaceDirectory) {}",
        path,
    )
    .unwrap();
    for wrong_side in [
        "impl fleet_contracts::WorkspaceDirectory for CoreDirectory {}",
        "mod requests {
            use fleet_contracts::{WorkspaceDirectory as Directory};
            impl Directory for CoreDirectory {}
        }",
    ] {
        let error = check_core_directory_placement(wrong_side, path).unwrap_err();
        assert!(
            error.to_string().contains(
                "collaboration-central-core must consume WorkspaceDirectory through fleet-contracts"
            ),
            "{error}"
        );
    }
}
