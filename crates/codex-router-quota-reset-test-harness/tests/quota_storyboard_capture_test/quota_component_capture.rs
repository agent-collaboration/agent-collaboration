//! Static normal component tour. Pending observations use the existing live CUA owner.
use super::quota_component_tape::{
    NORMAL_CHECKPOINTS, build_failure_retention_control_tape, build_first_frame_diagnostic_tape,
    build_normal_component_tape, validate_normal_checkpoints,
};
use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;

fn prepare_layer_destination(output: &Path) -> TestResult<()> {
    ensure(
        fs::metadata(output)?.dev() == fs::metadata(std::env::temp_dir())?.dev(),
        "VHS layer retention requires output and recorder temp on the same filesystem",
    )?;
    let diagnostic = output.join("diagnostic");
    fs::create_dir(&diagnostic)?;
    fs::set_permissions(diagnostic, fs::Permissions::from_mode(0o700))?;
    // VHS renames its entire recorder directory here. Leave the target absent.
    ensure(
        fs::symlink_metadata(output.join("diagnostic/frames"))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
        "raw-layer destination must be absent before recording",
    )
}

fn layer_dimensions(path: &Path) -> TestResult<(u32, u32)> {
    ensure(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "original raster layer must be a regular file, never a symlink",
    )?;
    let bytes = fs::read(path)?;
    ensure(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            && bytes.get(12..16) == Some(b"IHDR")
            && bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82"),
        "original raster layer must have a complete PNG envelope",
    )?;
    let width = u32::from_be_bytes(bytes.get(16..20).ok_or("layer width missing")?.try_into()?);
    let height = u32::from_be_bytes(
        bytes
            .get(20..24)
            .ok_or("layer height missing")?
            .try_into()?,
    );
    ensure(width > 100 && height > 100, "layer viewport is missing")?;
    Ok((width, height))
}

fn collect_original_layers(output: &Path) -> TestResult<()> {
    ensure(
        fs::symlink_metadata(output.join("diagnostic"))?
            .file_type()
            .is_dir(),
        "diagnostic parent must remain an owned directory",
    )?;
    let frames = output.join("diagnostic/frames");
    ensure(
        fs::symlink_metadata(&frames)?.file_type().is_dir(),
        "retained frame directory must be an original directory, never a symlink",
    )?;
    let mut pairs = BTreeMap::<u32, (Option<PathBuf>, Option<PathBuf>)>::new();
    for entry in fs::read_dir(&frames)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("raster layer name must be UTF-8")?;
        let (index, text_layer) = if let Some(index) = name.strip_prefix("frame-text-") {
            (index, true)
        } else if let Some(index) = name.strip_prefix("frame-cursor-") {
            (index, false)
        } else {
            return Err("unexpected file in original raster directory".into());
        };
        let digits = index
            .strip_suffix(".png")
            .ok_or("raster layer is not PNG")?;
        let index: u32 = digits.parse()?;
        ensure(
            index > 0 && digits == format!("{index:05}"),
            "raster layer must retain its original numbered filename",
        )?;
        let pair = pairs.entry(index).or_default();
        let slot = if text_layer { &mut pair.0 } else { &mut pair.1 };
        ensure(
            slot.replace(entry.path()).is_none(),
            "duplicate raster layer",
        )?;
    }
    ensure(!pairs.is_empty(), "VHS retained no original raster layers")?;
    let mut records = Vec::with_capacity(pairs.len());
    for (index, (text, cursor)) in pairs {
        let text = text.ok_or("original raster pair lacks its text layer")?;
        let cursor = cursor.ok_or("original raster pair lacks its cursor layer")?;
        let dimensions = layer_dimensions(&text)?;
        ensure(
            dimensions == layer_dimensions(&cursor)?,
            "original text and cursor layer dimensions must agree",
        )?;
        records.push(format!(
            "{{\"index\":{index},\"width\":{},\"height\":{},\"text_sha256\":{},\"cursor_sha256\":{}}}",
            dimensions.0, dimensions.1,
            encode_json_string(&digest_file(&text)?),
            encode_json_string(&digest_file(&cursor)?),
        ));
    }
    fs::write(
        output.join("diagnostic-layers.json"),
        format!(
            "{{\"kind\":\"original-vhs-raster-layers\",\"composed\":false,\"exact_failure_instant\":false,\"checkpoint_evidence\":false,\"png_validation\":\"envelope-and-paired-dimensions\",\"pairs\":[{}]}}\n",
            records.join(","),
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod layer_tests {
    use super::*;

    const BLANK_PNG: &[u8] = include_bytes!("fixtures/blank-startup.png");

    fn seeded_pair(output: &Path) -> TestResult<()> {
        prepare_layer_destination(output)?;
        fs::create_dir(output.join("diagnostic/frames"))?;
        // Existing checked PNG bytes test the collector only, never a VHS claim.
        for layer in ["text", "cursor"] {
            fs::write(
                output.join(format!("diagnostic/frames/frame-{layer}-00001.png")),
                BLANK_PNG,
            )?;
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn raw_layer_collector_preserves_bytes_and_reports_only_layer_evidence() -> TestResult<()>
    {
        let fixture = QuotaResetFixture::create().await?;
        seeded_pair(fixture.root())?;
        collect_original_layers(fixture.root())?;
        let manifest = fs::read_to_string(fixture.root().join("diagnostic-layers.json"))?;
        ensure(
            manifest.contains("\"composed\":false"),
            "layers claimed composition",
        )?;
        ensure(
            manifest.contains("\"exact_failure_instant\":false"),
            "layers claimed exact failure time",
        )?;
        ensure(
            manifest.contains("\"checkpoint_evidence\":false"),
            "layers claimed checkpoint evidence",
        )?;
        let expected = format!("{:x}", Sha256::digest(BLANK_PNG));
        ensure(
            manifest.matches(&expected).count() == 2,
            "both layer hashes must survive",
        )?;
        for layer in ["text", "cursor"] {
            ensure(
                fs::read(
                    fixture
                        .root()
                        .join(format!("diagnostic/frames/frame-{layer}-00001.png")),
                )? == BLANK_PNG,
                "collector altered original layer bytes",
            )?;
        }
        ensure(
            !fixture.root().join("component-receipt.json").exists(),
            "layers manufactured a normal receipt",
        )?;
        ensure(
            prepare_layer_destination(fixture.root()).is_err(),
            "existing destination accepted",
        )?;
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn raw_layer_collector_rejects_missing_malformed_and_linked_pairs() -> TestResult<()> {
        let fixture = QuotaResetFixture::create().await?;
        for rejected in [
            "absent",
            "empty",
            "unpaired",
            "malformed",
            "symlink",
            "unknown",
        ] {
            let output = fixture.root().join(rejected);
            fs::create_dir(&output)?;
            prepare_layer_destination(&output)?;
            let frames = output.join("diagnostic/frames");
            if rejected != "absent" {
                fs::create_dir(&frames)?;
            }
            if !matches!(rejected, "absent" | "empty") {
                fs::write(frames.join("frame-text-00001.png"), BLANK_PNG)?;
                match rejected {
                    "unpaired" => (),
                    "malformed" => fs::write(frames.join("frame-cursor-00001.png"), b"bad PNG")?,
                    "symlink" => std::os::unix::fs::symlink(
                        frames.join("frame-text-00001.png"),
                        frames.join("frame-cursor-00001.png"),
                    )?,
                    "unknown" => fs::write(frames.join("unattributed.png"), BLANK_PNG)?,
                    _ => return Err("unknown collector rejection scenario".into()),
                }
            }
            ensure(
                collect_original_layers(&output).is_err(),
                "collector accepted absent or unattributed raster layers",
            )?;
            ensure(
                !output.join("diagnostic-layers.json").exists(),
                "rejected layers acquired a valid manifest",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComponentCaptureJourney {
    NormalTour,
    FirstFrameDiagnostic,
    FailureRetentionControl,
}

pub(crate) async fn run_normal_component_capture(columns: u16, rows: u16) -> TestResult<()> {
    run_component_capture(columns, rows, ComponentCaptureJourney::NormalTour).await
}

pub(crate) async fn run_first_frame_component_diagnostic() -> TestResult<()> {
    run_component_capture(48, 12, ComponentCaptureJourney::FirstFrameDiagnostic).await
}

pub(crate) async fn run_component_failure_retention_control() -> TestResult<()> {
    run_component_capture(32, 24, ComponentCaptureJourney::FailureRetentionControl).await
}

async fn run_component_capture(
    columns: u16,
    rows: u16,
    journey: ComponentCaptureJourney,
) -> TestResult<()> {
    let binary = fs::canonicalize(
        std::env::var_os("QUOTA_STORYBOARD_COMPONENT_FIXTURE_BINARY")
            .ok_or("declared component libtest binary required")?,
    )?;
    ensure(binary.is_file(), "component libtest binary missing")?;
    ensure(
        std::env::var("QUOTA_STORYBOARD_COMPONENT_FIXTURE_SCENARIO").as_deref() == Ok("normal"),
        "static component tour selects only the normal scenario",
    )?;
    let tools = CaptureTools::from_environment()?;
    ensure_capture_running(tools.stop_file.as_deref())?;
    let output_root = fs::canonicalize(
        std::env::var_os("QUOTA_STORYBOARD_OUTPUT")
            .ok_or("fresh component output root required")?,
    )?;
    let purpose = match journey {
        ComponentCaptureJourney::NormalTour => "normal",
        ComponentCaptureJourney::FirstFrameDiagnostic => "first-frame-diagnostic",
        ComponentCaptureJourney::FailureRetentionControl => "failure-retention-control",
    };
    let output = output_root.join(format!("quota-component-{purpose}-{columns}x{rows}"));
    fs::create_dir(&output)?;
    fs::set_permissions(&output, fs::Permissions::from_mode(0o700))?;
    prepare_layer_destination(&output)?;
    fs::write(
        output.join("fixture-command.json"),
        format!(
            "{{\"binary\":{},\"output\":{},\"temporary_directory\":{},\"surface\":\"quota-component\",\"component_fixture_scenario\":\"normal\",\"component_fixture_receipt\":{},\"ttyd\":{},\"dynamic_library_path\":{}}}\n",
            encode_json_string(&binary.to_string_lossy()),
            encode_json_string(&output.to_string_lossy()),
            encode_json_string(&std::env::temp_dir().to_string_lossy()),
            encode_json_string(&output.join("fixture-finished").to_string_lossy()),
            encode_json_string(&tools.ttyd.to_string_lossy()),
            encode_json_string(&tools.dynamic_library_path),
        ),
    )?;
    tools.write_child_adapter_files(&output)?;
    validate_normal_checkpoints(&NORMAL_CHECKPOINTS)?;
    let tape = match journey {
        ComponentCaptureJourney::NormalTour => build_normal_component_tape(columns, rows)?,
        ComponentCaptureJourney::FirstFrameDiagnostic => build_first_frame_diagnostic_tape(),
        ComponentCaptureJourney::FailureRetentionControl => build_failure_retention_control_tape(),
    };
    fs::write(output.join("quota.tape"), tape)?;
    let parsed = Command::new(&tools.vhs)
        .arg("validate")
        .arg(output.join("quota.tape"))
        .output()?;
    fs::write(output.join("tape-validation.stdout"), &parsed.stdout)?;
    fs::write(output.join("tape-validation.stderr"), &parsed.stderr)?;
    ensure(
        parsed.status.success(),
        "component tape rejected before any UI startup",
    )?;
    let mut capture =
        tools.start_capture_with_owner(&output, CapturedChildOwnership::RetainedPythonChild)?;
    let status = capture.wait(Duration::from_secs(240))?;
    capture.release_process_group()?;
    let layers = collect_original_layers(&output);
    if let Err(error) = &layers {
        fs::write(output.join("diagnostic-layer-error.txt"), error.to_string())?;
    }
    if journey == ComponentCaptureJourney::FailureRetentionControl {
        ensure(!status.success(), "impossible wait unexpectedly succeeded")?;
        let stderr = fs::read_to_string(output.join("vhs.stderr"))?;
        let expected = format!(
            "failed to execute command: timeout waiting for \"Screen {IMPOSSIBLE_SCREEN}\""
        );
        ensure(
            stderr.contains(&expected),
            "failure control must reach the exact fixed impossible screen wait",
        )?;
        layers?;
        ensure_owned_child_stopped(&output)?;
        ensure(
            !output.join("component-quota-browse.png").exists()
                && !output.join("component-receipt.json").exists(),
            "raw layers must not masquerade as exported scenes or a normal receipt",
        )?;
        fs::write(
            output.join("failure-retention-control-receipt.json"),
            format!(
                "{{\"kind\":\"compiled-quota-natural-error-layer-control\",\"complete\":false,\"columns\":32,\"rows\":24,\"binary_sha256\":{},\"tape_sha256\":{},\"layers_manifest_sha256\":{},\"vhs_exit\":{},\"owned_children_stopped\":true,\"effects_observed\":false}}\n",
                encode_json_string(&digest_file(&binary)?),
                encode_json_string(&digest_file(&output.join("quota.tape"))?),
                encode_json_string(&digest_file(&output.join("diagnostic-layers.json"))?),
                status
                    .code()
                    .ok_or("natural failure control must have an exit code")?,
            ),
        )?;
        return Ok(());
    }
    ensure(
        status.success(),
        "component VHS tour failed; retain raw output",
    )?;
    layers?;
    ensure_owned_child_stopped(&output)?;
    ensure(
        fs::read_to_string(output.join("quota-child.viewport"))? == format!("{columns} {rows}"),
        "component viewport mismatch",
    )?;
    ensure(
        fs::read_to_string(output.join("quota-child.exit"))?.trim() == "0"
            && fs::read_to_string(output.join("component-process.exit"))?.trim() == "0",
        "component child or durable completion failed",
    )?;
    ensure(
        fs::read(output.join("fixture-finished"))?
            == b"QUOTA_COMPONENT_FIXTURE_FINISHED scenario=Normal released=false external_requests=0 persisted_writes=0\n",
        "component completion receipt mismatch",
    )?;
    let transcript = fs::read_to_string(output.join("screens.txt"))?;
    ensure(
        !transcript.contains("panicked at"),
        "component fixture panicked",
    )?;
    for canary in FORBIDDEN_TERMINAL_CANARIES {
        ensure(
            !transcript.to_lowercase().contains(canary),
            "component leaked fixture canary",
        )?;
    }
    if journey == ComponentCaptureJourney::FirstFrameDiagnostic {
        let image = output.join("component-quota-browse.png");
        validate_png(&image)?;
        // Deliberately no full-image oracle: the original is for investigating
        // the phone layout. The normal receipt and its42+12 checks stay below.
        fs::write(
            output.join("first-frame-diagnostic-receipt.json"),
            format!(
                "{{\"kind\":\"compiled-quota-first-frame-diagnostic\",\"complete\":false,\"columns\":{columns},\"rows\":{rows},\"binary_sha256\":{},\"png_sha256\":{},\"tape_sha256\":{},\"child_exit\":0,\"external_requests\":0,\"persisted_writes\":0,\"owned_children_stopped\":true}}\n",
                encode_json_string(&digest_file(&binary)?),
                encode_json_string(&digest_file(&image)?),
                encode_json_string(&digest_file(&output.join("quota.tape"))?),
            ),
        )?;
        return Ok(());
    }
    for checkpoint in NORMAL_CHECKPOINTS {
        let image = output.join(format!("{checkpoint}.png"));
        validate_png(&image)?;
        tools.verify_image(&output, checkpoint, &image, false)?;
    }
    let blank = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/quota_storyboard_capture_test/fixtures/blank-startup.png"
    ));
    tools.verify_image(&output, "component-quota-policy-saved", blank, true)?;
    tools.verify_image(
        &output,
        "component-quota-policy-saved",
        &output.join("component-quota-policy-cancelled.png"),
        true,
    )?;
    tools.verify_image(
        &output,
        "component-quota-floor-saved",
        &output.join("component-quota-floor-cancelled.png"),
        true,
    )?;
    tools.verify_image(
        &output,
        "component-quota-floor-saved",
        &output.join("component-quota-floor-draft.png"),
        true,
    )?;
    tools.verify_image(
        &output,
        "component-quota-credits-refreshed-balance",
        &output.join("component-quota-credits-balance.png"),
        true,
    )?;
    tools.verify_image(
        &output,
        "component-quota-returned",
        &output.join("component-quota-reset-result.png"),
        true,
    )?;
    for (checkpoint, wrong_checkpoint) in [
        ("component-quota-help-bottom", "component-quota-help-top"),
        (
            "component-quota-browse-credit-window",
            "component-quota-next-account",
        ),
        (
            "component-quota-browse-pace-heading",
            "component-quota-next-account",
        ),
        (
            "component-quota-browse-pace-value",
            "component-quota-reset-result",
        ),
        (
            "component-quota-details-bottom",
            "component-quota-details-top",
        ),
        (
            "component-quota-reset-result-bottom",
            "component-quota-confirm-yes",
        ),
    ] {
        tools.verify_image(
            &output,
            checkpoint,
            &output.join(format!("{wrong_checkpoint}.png")),
            true,
        )?;
    }
    let images = NORMAL_CHECKPOINTS
        .iter()
        .map(|checkpoint| {
            Ok(format!(
                "{{\"checkpoint\":{},\"png_sha256\":{}}}",
                encode_json_string(checkpoint),
                encode_json_string(&digest_file(&output.join(format!("{checkpoint}.png")))?),
            ))
        })
        .collect::<TestResult<Vec<_>>>()?
        .join(",");
    fs::write(
        output.join("component-receipt.json"),
        format!(
            "{{\"kind\":\"compiled-quota-component-normal\",\"columns\":{columns},\"rows\":{rows},\"binary_sha256\":{},\"verifier_sha256\":{},\"checkpoint_source_sha256\":{},\"recognition_source_sha256\":{},\"driver_source_sha256\":{},\"tape_source_sha256\":{},\"fixed_child_sha256\":{},\"tape_sha256\":{},\"child_exit\":0,\"external_requests\":0,\"persisted_writes\":0,\"owned_children_stopped\":true,\"images\":[{images}]}}\n",
            encode_json_string(&digest_file(&binary)?),
            encode_json_string(&digest_file(&tools.image_verifier)?),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("verify_screen_checkpoint.swift"))
            )),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("verify_screen_image.swift"))
            )),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("quota_component_capture.rs"))
            )),
            encode_json_string(&format!(
                "{:x}",
                Sha256::digest(include_bytes!("quota_component_tape.rs"))
            )),
            encode_json_string(&digest_file(&output.join("fixed_quota_child.py"))?),
            encode_json_string(&digest_file(&output.join("quota.tape"))?),
        ),
    )?;
    Ok(())
}
