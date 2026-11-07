//! Schema export preparation driven through a Codex stand-in executable.
use super::*;
use std::os::unix::fs::PermissionsExt as _;

/// Writes a Codex stand-in whose `app-server generate-json-schema --out <dir>`
/// writes a minimal valid schema into `<dir>` and records `<dir>` in `record`.
fn codex_stand_in(directory: &Path, record: &Path) -> std::io::Result<std::path::PathBuf> {
    let executable = directory.join("codex-stand-in");
    let mut definitions = serde_json::Map::new();
    for name in ["ThreadRead", "ThreadStart", "TurnStart"] {
        definitions.insert(
            format!("{name}Params"),
            serde_json::json!({"type": "object"}),
        );
        definitions.insert(
            format!("{name}Response"),
            serde_json::json!({"type": "object"}),
        );
    }
    let schema = serde_json::json!({"definitions": {"v2": definitions}});
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$5\" >> '{}'\ncat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\n",
            record.display()
        ),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    Ok(executable)
}

fn private_directory() -> std::io::Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}

#[tokio::test]
async fn schema_export_folder_is_named_with_a_uuidv7() -> Result<(), Box<dyn std::error::Error>> {
    let root = private_directory()?;
    let record = root.path().join("export-folders");
    let executable = codex_stand_in(root.path(), &record)?;
    let schema_directory = root.path().join("collaboration");
    let identity = codex_native_integration::executable_identity(&executable).await?;
    let mut plan = AppServerLaunchPlan::new(
        ChildCommandSpec::new(executable.clone()),
        identity,
        "fixture".to_owned(),
    )
    .with_schema_directory(schema_directory.clone());

    plan.prepare_schema().await;

    if plan.prepared_schema_export().is_none() {
        return Err("the stand-in's schema export was not prepared".into());
    }
    let recorded = std::fs::read_to_string(&record)?;
    let folder = recorded
        .lines()
        .next()
        .ok_or("stand-in saw no --out folder")?;
    let suffix = Path::new(folder)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("native-schema-export-"))
        .ok_or("export folder name")?;
    if uuid::Uuid::parse_str(suffix)?.get_version_num() != 7 {
        return Err(format!("export folder {folder} is not named with a UUIDv7").into());
    }
    Ok(())
}
