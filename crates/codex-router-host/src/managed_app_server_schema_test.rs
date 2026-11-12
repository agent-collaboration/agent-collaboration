//! Schema export preparation and schema bundle retention, driven through Codex
//! stand-in executables and the Host's own publication at startup (J9).
use super::*;
use std::os::unix::fs::PermissionsExt as _;

const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// What a Codex stand-in does when asked to export its schema.
#[derive(Clone, Copy)]
enum StandIn<'a> {
    /// Writes a valid schema whose content (and digest) depends on `variant`.
    Exports { variant: &'a str },
    /// Writes part of a schema into the folder, then fails.
    FailsAfterWriting,
}

/// Writes a Codex stand-in that records every `--out <dir>` it is given in
/// `record`, then behaves as `behaviour` says.
fn codex_stand_in(
    directory: &Path,
    name: &str,
    record: &Path,
    behaviour: StandIn<'_>,
) -> std::io::Result<std::path::PathBuf> {
    let executable = directory.join(name);
    let body = match behaviour {
        StandIn::Exports { variant } => {
            let mut definitions = serde_json::Map::new();
            for operation in ["ThreadRead", "ThreadStart", "TurnStart"] {
                definitions.insert(
                    format!("{operation}Params"),
                    serde_json::json!({"type": "object"}),
                );
                definitions.insert(
                    format!("{operation}Response"),
                    serde_json::json!({"type": "object", "description": variant}),
                );
            }
            let schema = serde_json::json!({"definitions": {"v2": definitions}});
            format!(
                "cat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\n"
            )
        }
        StandIn::FailsAfterWriting => {
            "printf '{' > \"$5/codex_app_server_protocol.schemas.json\"\nexit 3\n".to_owned()
        }
    };
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$5\" >> '{}'\n{body}",
            record.display()
        ),
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    Ok(executable)
}

fn private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Runs the real `prepare_schema` for `executable` against `schema_directory`.
async fn prepare(
    executable: &Path,
    schema_directory: &Path,
) -> Result<AppServerLaunchPlan, Box<dyn std::error::Error>> {
    let identity = codex_native_integration::executable_identity(executable).await?;
    let mut plan = AppServerLaunchPlan::new(
        ChildCommandSpec::new(executable.to_owned()),
        identity,
        "fixture".to_owned(),
    )
    .with_schema_directory(schema_directory.to_owned());
    plan.prepare_schema().await;
    Ok(plan)
}

fn recorded_folders(record: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    Ok(std::fs::read_to_string(record)?
        .lines()
        .map(std::path::PathBuf::from)
        .collect())
}

fn export_folders(directory: &Path) -> std::io::Result<Vec<String>> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with("native-schema-export-") {
            folders.push(name);
        }
    }
    Ok(folders)
}

fn bundle_files(directory: &Path) -> std::io::Result<std::collections::BTreeSet<String>> {
    let mut bundles = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(directory)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.len() == 69 && name.ends_with(".json") && name != "service.json" {
            bundles.insert(name);
        }
    }
    Ok(bundles)
}

fn bundle_file_name(bundle: &codex_native_integration::NativeSchemaBundle) -> String {
    let mut name: String = bundle
        .digest()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    name.push_str(".json");
    name
}

fn last_use(path: &Path) -> std::io::Result<std::time::SystemTime> {
    std::fs::metadata(path)?.modified()
}

#[tokio::test]
async fn schema_export_folder_is_named_with_a_uuidv7() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    private_directory(root.path())?;
    let record = root.path().join("export-folders");
    let executable = codex_stand_in(
        root.path(),
        "codex-stand-in",
        &record,
        StandIn::Exports { variant: "a" },
    )?;

    let plan = prepare(&executable, &root.path().join("collaboration")).await?;

    if plan.prepared_schema_export().is_none() {
        return Err("the stand-in's schema export was not prepared".into());
    }
    let folders = recorded_folders(&record)?;
    let folder = folders.first().ok_or("stand-in saw no --out folder")?;
    let suffix = folder
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("native-schema-export-"))
        .ok_or("export folder name")?;
    if uuid::Uuid::parse_str(suffix)?.get_version_num() != 7 {
        return Err(format!(
            "export folder {} is not named with a UUIDv7",
            folder.display()
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn a_prepared_export_leaves_no_folder_behind() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    private_directory(root.path())?;
    let record = root.path().join("export-folders");
    let executable = codex_stand_in(
        root.path(),
        "codex-stand-in",
        &record,
        StandIn::Exports { variant: "a" },
    )?;
    let schema_directory = root.path().join("collaboration");

    let plan = prepare(&executable, &schema_directory).await?;

    if plan.prepared_schema_export().is_none() {
        return Err("the export must still be prepared from the folder it read".into());
    }
    let folders = recorded_folders(&record)?;
    if folders.len() != 1 || folders.iter().any(|folder| folder.exists()) {
        return Err(format!("export folders left behind: {folders:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn a_failed_export_leaves_no_folder_behind() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    private_directory(root.path())?;
    let record = root.path().join("export-folders");
    let executable = codex_stand_in(
        root.path(),
        "codex-stand-in",
        &record,
        StandIn::FailsAfterWriting,
    )?;
    let schema_directory = root.path().join("collaboration");

    let plan = prepare(&executable, &schema_directory).await?;

    if plan.prepared_schema_export().is_some() {
        return Err("a failed generator cannot produce an export".into());
    }
    let folders = recorded_folders(&record)?;
    if folders.len() != 1 || folders.iter().any(|folder| folder.exists()) {
        return Err(format!("failed export folders left behind: {folders:?}").into());
    }
    Ok(())
}

#[tokio::test]
async fn folders_left_by_an_interrupted_start_are_removed_at_the_next_prepare()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    private_directory(root.path())?;
    let schema_directory = root.path().join("collaboration");
    private_directory(&schema_directory)?;
    let abandoned =
        schema_directory.join("native-schema-export-01a1b6ff-fa59-7992-8d00-d83979384f53");
    std::fs::create_dir(&abandoned)?;
    std::fs::write(
        abandoned.join("codex_app_server_protocol.schemas.json"),
        b"{",
    )?;
    let unrelated = schema_directory.join("router-sessions");
    std::fs::create_dir(&unrelated)?;
    let record = root.path().join("export-folders");
    let executable = codex_stand_in(
        root.path(),
        "codex-stand-in",
        &record,
        StandIn::Exports { variant: "a" },
    )?;

    let _plan = prepare(&executable, &schema_directory).await?;

    if abandoned.exists() {
        return Err("an abandoned export folder survived the next prepare".into());
    }
    if !unrelated.is_dir() {
        return Err("only export folders may be removed".into());
    }
    Ok(())
}

/// J9: across several Host starts with changing Codex stand-ins and an
/// interrupted export, no export folder survives, and the bundles left are the
/// one in use plus those used within 30 days.
#[tokio::test]
async fn host_starts_keep_the_bundle_in_use_and_bundles_used_within_thirty_days()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    private_directory(root.path())?;
    let directory = root.path().join("collaboration");
    private_directory(&directory)?;
    let record = root.path().join("export-folders");
    let codex_a = codex_stand_in(
        root.path(),
        "codex-a",
        &record,
        StandIn::Exports { variant: "a" },
    )?;
    let codex_b = codex_stand_in(
        root.path(),
        "codex-b",
        &record,
        StandIn::Exports { variant: "b" },
    )?;
    let broken = codex_stand_in(
        root.path(),
        "codex-broken",
        &record,
        StandIn::FailsAfterWriting,
    )?;
    let now = std::time::SystemTime::now();

    // Bundles from earlier days: A will be used again; "stale" was last used
    // 31 days ago and "recent" 10 days ago, and neither is used again.
    let seed = prepare(&codex_a, &directory).await?;
    let seed_export = seed.prepared_schema_export().ok_or("stand-in A exports")?;
    let a = seed_export.bundle();
    a.publish(&directory, now - 40 * DAY)?;
    let stale = codex_native_integration::NativeSchemaBundle::from_documents(
        std::collections::BTreeMap::from([(
            "codex_app_server_protocol.schemas.json".to_owned(),
            br#"{"definitions":{"v2":{"stale":{}}}}"#.to_vec(),
        )]),
    )?;
    let recent = codex_native_integration::NativeSchemaBundle::from_documents(
        std::collections::BTreeMap::from([(
            "codex_app_server_protocol.schemas.json".to_owned(),
            br#"{"definitions":{"v2":{"recent":{}}}}"#.to_vec(),
        )]),
    )?;
    stale.publish(&directory, now - 31 * DAY)?;
    recent.publish(&directory, now - 10 * DAY)?;
    // An export a crashed start never cleaned up.
    let crashed = directory.join("native-schema-export-01a1b6ff-0000-7000-8000-00000000c0de");
    std::fs::create_dir(&crashed)?;
    // An interrupted generator leaves a partly written schema behind.
    std::fs::write(
        crashed.join("codex_app_server_protocol.schemas.json"),
        br#"{"definitions":{"v2":{"ThreadReadParams":{"type":"obj"#,
    )?;

    let mut b_name = None;
    for (start, executable) in [codex_a.as_path(), broken.as_path(), codex_b.as_path()]
        .into_iter()
        .enumerate()
    {
        // Act: the Host prepares the export, then its runtime publishes it.
        let plan = prepare(executable, &directory).await?;
        let mut inputs = crate::CollaborationRuntimeInputs {
            directory: directory.clone(),
            codex_home: directory.clone(),
            backend_socket: directory.join("backend.sock"),
            mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
            native_schema: None,
            peer_registry_directory: Some(directory.join("claude-sessions")),
            remote_control_server_name: None,
            owner_human_id: None,
        };
        inputs.native_schema = plan.prepared_schema_export();
        if let Some(export) = &inputs.native_schema
            && start == 2
        {
            b_name = Some(bundle_file_name(export.bundle()));
        }
        let runtime = crate::CollaborationRuntime::start(inputs).await?;
        runtime.shutdown().await?;

        if !export_folders(&directory)?.is_empty() {
            return Err(format!("start {start} left export folders behind").into());
        }
    }

    // Assert: B is in use; A was used again today; "recent" is within 30 days.
    let b_name = b_name.ok_or("stand-in B exports")?;
    let expected: std::collections::BTreeSet<String> =
        [b_name, bundle_file_name(a), bundle_file_name(&recent)]
            .into_iter()
            .collect();
    let left = bundle_files(&directory)?;
    if left != expected {
        return Err(format!("bundles left {left:?}, expected {expected:?}").into());
    }
    if last_use(&directory.join(bundle_file_name(a)))? < now {
        return Err("reusing bundle A must record its use".into());
    }
    if !recorded_folders(&record)?
        .iter()
        .all(|folder| !folder.exists())
    {
        return Err("an export folder the stand-ins wrote still exists".into());
    }
    Ok(())
}
