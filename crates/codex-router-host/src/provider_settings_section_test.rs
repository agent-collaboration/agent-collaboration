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

/// Oracle: D1 R18, "work roots are one list in the Router's settings.jsonc.
/// By default: ~/code/projects, ~/dev/memory-logs, ~/dev/session-logs; the
/// toolchain caches; system temp, all of /tmp/agents/": a file written before
/// the list existed gets those defaults, expanded under the owner's home and
/// the system temp folder.
#[test]
fn a_file_without_work_roots_gets_the_default_list_expanded() {
    // Arrange
    let providers = load_providers(CLAUDE_WITHOUT_CACHE_WARM, CURSOR_WITHOUT_CACHE_WARM)
        .expect("a file written before workRoots existed stays valid");

    // Act
    let roots = providers.work_roots(
        std::path::Path::new("/Users/owner"),
        std::path::Path::new("/var/folders/t"),
    );

    // Assert
    let expected: Vec<std::path::PathBuf> = [
        "/Users/owner/code/projects",
        "/Users/owner/dev/memory-logs",
        "/Users/owner/dev/session-logs",
        "/Users/owner/.cargo",
        "/Users/owner/.rustup",
        "/Users/owner/Library/Caches",
        "/Users/owner/.cache",
        "/Users/owner/Library/pnpm",
        "/var/folders/t",
        "/tmp/agents",
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .collect();
    assert_eq!(roots, expected);
}

/// Oracle: D1 R18's list, checked when read: an entry that is neither
/// absolute nor under ~/ or $TMPDIR is refused, naming it.
#[test]
fn a_relative_work_root_is_refused_naming_it() {
    // Arrange
    let root = tempfile::tempdir().expect("router root");
    std::fs::write(
        router_settings_path(root.path()),
        format!(
            r#"{{"providers": {{"version":1,"providers":{{"claude":{CLAUDE_WITHOUT_CACHE_WARM},"cursor":{CURSOR_WITHOUT_CACHE_WARM}}},"workRoots":["code/projects"]}}}}"#
        ),
    )
    .expect("settings fixture");

    // Act
    let loaded = load_or_create_provider_settings(root.path());

    // Assert
    let error = loaded.expect_err("a relative root is refused").to_string();
    assert!(error.contains("code/projects"), "{error}");
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
