//! Test-support ownership and target layout use manifest facts, never shipping features.
use super::{CargoPackage, DependencyKind};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[path = "source_path_policy.rs"]
mod source_path_policy;

type PolicyResult = Result<(), Box<dyn std::error::Error>>;

pub(super) fn check_workspace(
    root: &Path,
    packages: &[CargoPackage],
    members: &BTreeSet<String>,
) -> PolicyResult {
    let workspace: Vec<_> = packages
        .iter()
        .filter(|package| members.contains(&package.id))
        .collect();
    for package in &workspace {
        check_layout(package)?;
        check_support_dependencies(package)?;
    }
    source_path_policy::check_source_paths(root, &workspace)
}

fn check_layout(package: &CargoPackage) -> PolicyResult {
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(&package.manifest_path)?)?;
    if manifest
        .get("package")
        .and_then(|value| value.get("autotests"))
        != Some(&toml::Value::Boolean(false))
    {
        return Err(format!("{}: package.autotests must be false", package.name).into());
    }
    let home = package
        .manifest_path
        .parent()
        .ok_or("manifest has no parent")?;
    if !home.join("tests/README.md").is_file() {
        return Err(format!("{}: tests/README.md is required", package.name).into());
    }
    let tests: Vec<_> = package
        .targets
        .iter()
        .filter(|target| target.kind.iter().any(|kind| kind == "test"))
        .collect();
    let mut names = BTreeSet::new();
    for target in tests {
        if !matches!(target.name.as_str(), "integration" | "e2e")
            || !names.insert(target.name.as_str())
            || target.src_path != home.join("tests").join(&target.name).join("main.rs")
        {
            return Err(format!(
                "{}: test targets must be at most one tests/integration/main.rs and one tests/e2e/main.rs; found {} at {}",
                package.name, target.name, target.src_path.display()
            ).into());
        }
        let source = syn::parse_file(&fs::read_to_string(&target.src_path)?)?;
        if source.items.is_empty() {
            return Err(format!("{}: {} aggregate is empty", package.name, target.name).into());
        }
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

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
