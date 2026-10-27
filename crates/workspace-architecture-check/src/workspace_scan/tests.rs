#![expect(
    clippy::panic_in_result_fn,
    reason = "test assertions report contract failures; Result propagates setup errors"
)]

use super::{TargetSelector, WorkspaceMetadata, workspace_targets};

#[test]
fn metadata_selects_members_libs_and_all_bin_only_targets() -> Result<(), serde_json::Error> {
    let metadata: WorkspaceMetadata = serde_json::from_str(
        r#"{
  "workspace_members":["lib-id","bin-id","cdylib-id"],
  "packages":[
    {"id":"external-id","name":"external","targets":[{"name":"external","kind":["lib"]}]},
    {"id":"lib-id","name":"library","targets":[
      {"name":"library","kind":["lib"]},
      {"name":"other-bin","kind":["bin"]},
      {"name":"example","kind":["example"]}]},
    {"id":"bin-id","name":"bin-only","targets":[
      {"name":"first-bin","kind":["bin"]},
      {"name":"second-bin","kind":["bin"]},
      {"name":"integration","kind":["test"]}]},
    {"id":"cdylib-id","name":"cdylib","targets":[{"name":"dynamic","kind":["cdylib"]}]}
  ]
}"#,
    )?;
    let targets = workspace_targets(metadata);
    let summary: Vec<_> = targets
        .iter()
        .map(|target| {
            (
                target.package_name.as_str(),
                target.crate_name.as_str(),
                match &target.selector {
                    TargetSelector::Library => "lib",
                    TargetSelector::Binary(name) => name,
                },
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("bin-only", "first_bin", "first-bin"),
            ("bin-only", "second_bin", "second-bin"),
            ("cdylib", "dynamic", "lib"),
            ("library", "library", "lib"),
        ]
    );
    Ok(())
}
