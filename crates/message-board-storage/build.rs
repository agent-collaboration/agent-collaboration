#[path = "src/project_migration_policy.rs"]
mod project_migration_policy;

fn main() {
    println!("cargo:rerun-if-env-changed=DATABASE_URL");
    // Stock SQLx preparation supplies a SQLite URL to its whole dependency graph.
    // This package has only Turso queries, independently described and checked by
    // prepare-sqlx-turso.py; use their native cache during the stock preparation.
    if std::env::var("DATABASE_URL").is_ok_and(|url| url.starts_with("sqlite:")) {
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }
    if let Err(error) = validate_project_migrations() {
        eprintln!("project migration build policy: {error}");
        std::process::exit(1);
    }
}

fn validate_project_migrations() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new("project_migrations");
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "sql") {
            project_migration_policy::validate_migration_sql(&std::fs::read_to_string(&path)?)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}
