use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const CHILD_ENV: &str = "FLEET_WORKSPACE_SCHEMA_CONTROL_CHILD";
const COMPLETED: &str = "workspace schema rejected all four invalid queries";

#[test]
fn sqlx_rejects_queries_that_disagree_with_the_migrated_workspace_schema() {
    if env::var_os(CHILD_ENV).is_some() {
        let cases = trybuild::TestCases::new();
        cases.compile_fail("tests/integration/schema_controls/bad_column.rs");
        cases.compile_fail("tests/integration/schema_controls/wrong_arity.rs");
        cases.compile_fail("tests/integration/schema_controls/missing_field.rs");
        cases.compile_fail("tests/integration/schema_controls/wrong_type.rs");
        println!("{COMPLETED}");
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("workspace.sqlite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        fleet_storage_schema::workspace_migrations::workspace_migrator()
            .run(&pool)
            .await
            .unwrap();
        pool.close().await;
    });

    // The checked macros need the live schema in the compiler's environment.
    let (_, module) = module_path!().split_once("::").unwrap();
    let test =
        format!("{module}::sqlx_rejects_queries_that_disagree_with_the_migrated_workspace_schema");
    let mut child = Command::new(env::current_exe().unwrap());
    child
        .args(["--exact", &test, "--nocapture"])
        .env(CHILD_ENV, "1")
        .env("DATABASE_URL", format!("sqlite://{}", database.display()))
        .env("SQLX_OFFLINE", "false");
    if let Some(target) = env::var_os("CARGO_TARGET_DIR").map(PathBuf::from)
        && target.is_relative()
    {
        // Cargo was launched from the workspace root. Trybuild runs from the crate.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        child.env(
            "CARGO_TARGET_DIR",
            root.canonicalize().unwrap().join(target),
        );
    }
    let output = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains(COMPLETED),
        "schema-control child failed ({})\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
