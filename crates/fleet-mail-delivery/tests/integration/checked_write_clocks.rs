use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(serde::Deserialize)]
struct CargoPackage {
    name: String,
    manifest_path: PathBuf,
}

fn durable_step_query_directory(manifest_directory: &Path) -> PathBuf {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(manifest_directory.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata
        .packages
        .into_iter()
        .filter(|package| package.name == "durable-steps")
        .collect::<Vec<_>>();
    assert_eq!(
        packages.len(),
        1,
        "expected one pinned durable-steps package"
    );
    packages
        .first()
        .unwrap()
        .manifest_path
        .parent()
        .unwrap()
        .join("queries")
}

fn sql_files(directory: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            paths.extend(sql_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "sql") {
            paths.push(path);
        }
    }
    paths
}

#[test]
fn checked_write_and_identity_queries_never_evaluate_the_sql_wall_clock() {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates_directory = manifest_directory.parent().unwrap();
    for (package, directory) in [
        ("fleet-mail-delivery", manifest_directory.join("sql")),
        (
            "fleet-project-lease",
            crates_directory.join("fleet-project-lease/sql"),
        ),
        ("fleet-session", crates_directory.join("fleet-session/sql")),
        (
            "durable-steps",
            durable_step_query_directory(manifest_directory),
        ),
    ] {
        let mut writes = 0;
        for path in sql_files(&directory) {
            let sql = fs::read_to_string(&path).unwrap();
            let statement = sql
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join(" ")
                .trim_start()
                .to_ascii_lowercase();
            let write = ["insert", "update", "delete", "replace"]
                .iter()
                .any(|prefix| statement.starts_with(prefix));
            let name = path.file_stem().unwrap().to_string_lossy();
            if write || name.contains("token") || name.contains("id") {
                writes += 1;
                assert!(
                    !(statement.contains("strftime") && statement.contains("'now'")),
                    "{}: writes and identity/token queries must use the owning injected sources",
                    path.display()
                );
            }
        }
        assert!(
            writes > 0,
            "{package}: write-query discovery returned no subjects"
        );
    }
}
