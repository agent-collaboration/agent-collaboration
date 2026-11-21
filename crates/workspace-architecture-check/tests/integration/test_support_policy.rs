//! Test-support ownership uses manifest dependency kinds.
use super::{CargoPackage, DependencyKind};
use std::collections::BTreeSet;

type PolicyResult = Result<(), Box<dyn std::error::Error>>;

pub(super) fn check_workspace(
    packages: &[CargoPackage],
    members: &BTreeSet<String>,
) -> PolicyResult {
    for package in packages
        .iter()
        .filter(|package| members.contains(&package.id))
    {
        check_support_dependencies(package)?;
    }
    Ok(())
}

fn check_support_dependencies(package: &CargoPackage) -> PolicyResult {
    if package.name.ends_with("-test-support") {
        return Ok(());
    }
    for dependency in &package.dependencies {
        if dependency.name.ends_with("-test-support")
            && dependency.kind != Some(DependencyKind::Dev)
        {
            return Err(format!(
                "forbidden {:?} test-support dependency: {} -> {}",
                dependency.kind.unwrap_or(DependencyKind::Normal),
                package.name,
                dependency.name
            )
            .into());
        }
    }
    Ok(())
}

#[test]
fn central_engine_fixtures_require_a_dev_edge() {
    use super::ManifestDependency;

    for kind in [None, Some(DependencyKind::Build), Some(DependencyKind::Dev)] {
        let package = CargoPackage {
            id: "fleet-wakes".to_owned(),
            name: "fleet-wakes".to_owned(),
            dependencies: vec![ManifestDependency {
                name: "central-engine-test-support".to_owned(),
                kind,
            }],
        };
        assert_eq!(
            check_support_dependencies(&package).is_ok(),
            kind == Some(DependencyKind::Dev)
        );
    }
}
