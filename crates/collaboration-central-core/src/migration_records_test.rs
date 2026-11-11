use super::{MigrationSet, StoredMigrationRecord, installed_on_text, validate};
use runtime_primitives::SyncTime;

fn record() -> StoredMigrationRecord {
    StoredMigrationRecord {
        version: Some(1),
        description: Some("copy rules".into()),
        success: Some(1),
        checksum: Some(vec![7; 48]),
        execution_time: Some(0),
        installed_on: Some("2026-11-09 13:41:38".into()),
        version_class: Some("integer".into()),
        success_class: Some("integer".into()),
        checksum_class: Some("blob".into()),
        execution_class: Some("integer".into()),
        installed_class: Some("text".into()),
    }
}

#[test]
fn every_set_keeps_its_stable_name_and_its_own_table() {
    let mut tables = std::collections::BTreeSet::new();
    for set in MigrationSet::ALL {
        assert_eq!(MigrationSet::named(set.name()).ok(), Some(set));
        assert!(tables.insert(set.table()), "{} shares a table", set.name());
        assert_eq!(
            set.table(),
            format!("_sqlx_migrations_{}", set.name().replace('-', "_"))
        );
    }
    assert!(MigrationSet::named("central-copy-summary").is_err());
}

#[test]
fn installed_on_is_sqlx_timestamp_text() {
    // 2026-11-09T13:41:38.250Z
    let at = SyncTime::from_unix_millis(1_794_231_698_250);
    assert_eq!(
        installed_on_text(at).ok().as_deref(),
        Some("2026-11-09 13:41:38")
    );
}

#[test]
fn a_record_outside_sqlx_shape_is_refused_by_field() {
    assert!(validate(&record(), MigrationSet::FleetSyncReceiver).is_ok());
    for (field, broken) in [
        (
            "version",
            StoredMigrationRecord {
                version_class: Some("text".into()),
                ..record()
            },
        ),
        (
            "installed_on",
            StoredMigrationRecord {
                installed_on: Some("1794231698250".into()),
                ..record()
            },
        ),
        (
            "success",
            StoredMigrationRecord {
                success: Some(0),
                ..record()
            },
        ),
        (
            "checksum",
            StoredMigrationRecord {
                checksum_class: Some("text".into()),
                ..record()
            },
        ),
        (
            "execution_time",
            StoredMigrationRecord {
                execution_time: Some(-1),
                ..record()
            },
        ),
    ] {
        let refused = validate(&broken, MigrationSet::FleetSyncReceiver);
        assert!(
            matches!(&refused, Err(durable_steps::StoreError::Corrupt { field: named, .. })
                if named == &format!("_sqlx_migrations_fleet_sync_receiver.{field}")),
            "{field}: {refused:?}"
        );
    }
}
