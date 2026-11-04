use std::error::Error;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use workspace_architecture_check::{CycleBaseline, scan_workspace};

#[test]
#[ignore = "needs cargo-modules 0.28; run by CI static job"]
fn module_cycles_do_not_grow() -> Result<(), Box<dyn Error>> {
    // Root/baseline overrides let the exact gate scan an origin/main snapshot
    // and permanent negative-control fixtures without altering workspace code.
    let root = std::env::var_os("ARCHITECTURE_ROOT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        PathBuf::from,
    );
    let root = root.canonicalize()?;
    let baseline_path = std::env::var_os("ARCHITECTURE_BASELINE").map_or_else(
        || root.join("architecture/module-cycle-baseline.json"),
        PathBuf::from,
    );
    let jobs = match std::env::var("ARCHITECTURE_JOBS") {
        Ok(value) => value.parse::<NonZeroUsize>()?,
        Err(std::env::VarError::NotPresent) => NonZeroUsize::new(4).ok_or("invalid job count")?,
        Err(error) => return Err(error.into()),
    };
    let bless = match std::env::var("ARCHITECTURE_BLESS") {
        Ok(value) if value == "1" => true,
        Ok(value) => return Err(format!("ARCHITECTURE_BLESS must be 1, found {value:?}").into()),
        Err(std::env::VarError::NotPresent) => false,
        Err(error) => return Err(error.into()),
    };
    let scan = scan_workspace(&root, jobs)?;
    let observed = CycleBaseline::from_graphs(&scan.packages);
    eprintln!(
        "TOTAL: {:.3}s, {} packages, {} targets, jobs={jobs}",
        scan.elapsed.as_secs_f64(),
        scan.packages.len(),
        scan.target_count,
    );
    if baseline_path.try_exists()? {
        let baseline = CycleBaseline::read(&baseline_path)?;
        if let Err(violation) = baseline.compare(&scan.packages)
            && (!bless || violation.has_growth())
        {
            eprintln!("{violation}");
            return Err(violation.into());
        }
    } else if !bless {
        return Err(format!("baseline missing: {}", baseline_path.display()).into());
    }
    if bless {
        observed.write(&baseline_path)?;
        eprintln!("wrote {}", baseline_path.display());
    }
    eprintln!("module cycle ratchet PASS");
    Ok(())
}
