use super::*;

pub(crate) fn delivery_matrix_debug_profile_passes_strict_parser()
-> Result<(), codex_native_integration::DebugProfileError> {
    codex_native_integration::DebugCodexProfile::parse(DELIVERY_MATRIX_DEBUG_PROFILE, 43127)?;
    Ok(())
}
