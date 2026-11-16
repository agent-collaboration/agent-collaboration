//! V30: the cache-warm setting is optional, defaults on for Claude, and is checked by name.
use super::*;
use crate::{load_or_create_provider_settings, router_settings_path};

const CLAUDE_WITHOUT_CACHE_WARM: &str = r#"{"enabled":true,"executable":null,"arguments":[]}"#;
const CURSOR_WITHOUT_CACHE_WARM: &str = r#"{"enabled":true,"executable":null,"arguments":["acp"]}"#;

/// A router root whose `settings.jsonc` carries these two provider entries.
fn router_root_with(claude: &str, cursor: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("router root");
    let text = format!(
        r#"{{"providers": {{"version":1,"providers":{{"claude":{claude},"cursor":{cursor}}}}}}}"#
    );
    std::fs::write(router_settings_path(root.path()), text).expect("settings fixture");
    root
}

fn load_providers(
    claude: &str,
    cursor: &str,
) -> Result<ProviderConfigurationEntries, crate::RouterSettingsError> {
    let root = router_root_with(claude, cursor);
    load_or_create_provider_settings(root.path())
}

#[test]
fn existing_file_without_cache_warm_loads_with_claude_on_at_55_and_8() {
    let providers = load_providers(CLAUDE_WITHOUT_CACHE_WARM, CURSOR_WITHOUT_CACHE_WARM)
        .expect("a file written before the setting existed stays valid");

    assert_eq!(
        providers.claude().effective_cache_warm(),
        CacheWarmSetting {
            enabled: true,
            every_minutes: 55,
            for_hours: 8,
        }
    );
    assert_eq!(providers.cursor().cache_warm, None);
}

#[test]
fn explicit_cache_warm_on_the_claude_entry_is_read() {
    let claude = r#"{"enabled":true,"executable":null,"arguments":[],"cacheWarm":{"enabled":false,"everyMinutes":30,"forHours":2}}"#;

    let providers = load_providers(claude, CURSOR_WITHOUT_CACHE_WARM).expect("explicit setting");

    assert_eq!(
        providers.claude().effective_cache_warm(),
        CacheWarmSetting {
            enabled: false,
            every_minutes: 30,
            for_hours: 2,
        }
    );
}

#[test]
fn cache_warm_on_the_cursor_entry_is_refused_naming_the_field() {
    let cursor = r#"{"enabled":true,"executable":null,"arguments":["acp"],"cacheWarm":{"enabled":true,"everyMinutes":55,"forHours":8}}"#;

    let error = load_providers(CLAUDE_WITHOUT_CACHE_WARM, cursor)
        .expect_err("Cursor has no cache to keep warm")
        .to_string();

    assert!(error.contains("cacheWarm"), "{error}");
    assert!(error.contains("cursor"), "{error}");
}

#[test]
fn zero_every_minutes_is_refused_naming_the_field() {
    let claude = r#"{"enabled":true,"executable":null,"arguments":[],"cacheWarm":{"enabled":true,"everyMinutes":0,"forHours":8}}"#;

    let error = load_providers(claude, CURSOR_WITHOUT_CACHE_WARM)
        .expect_err("zero minutes would send continuously")
        .to_string();

    assert!(error.contains("everyMinutes"), "{error}");
}

#[test]
fn zero_for_hours_is_refused_naming_the_field() {
    let claude = r#"{"enabled":true,"executable":null,"arguments":[],"cacheWarm":{"enabled":true,"everyMinutes":55,"forHours":0}}"#;

    let error = load_providers(claude, CURSOR_WITHOUT_CACHE_WARM)
        .expect_err("zero hours would never warm")
        .to_string();

    assert!(error.contains("forHours"), "{error}");
}
