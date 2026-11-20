use super::central_migrator;

/// A non-test-support build returns `migrations/central` unchanged, so that
/// directory must hold exactly 0001-0004; test-support appends only the probe
/// fixture after it, so no probe migration can ship.
#[test]
fn production_history_is_the_four_approved_migrations_and_test_support_adds_only_the_probe() {
    let production = sqlx::migrate!("migrations/central")
        .iter()
        .map(|migration| (migration.version, migration.description.to_string()))
        .collect::<Vec<_>>();
    assert_eq!(
        production
            .iter()
            .map(|(version, _)| *version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "{production:?}"
    );
    assert!(
        production[3].1.contains("central work attempts"),
        "{production:?}"
    );

    let built = central_migrator()
        .iter()
        .map(|migration| (migration.version, migration.description.to_string()))
        .collect::<Vec<_>>();

    #[cfg(feature = "test-support")]
    assert_eq!(
        built,
        production
            .iter()
            .cloned()
            .chain([(5, "engine probe fixture".to_owned())])
            .collect::<Vec<_>>()
    );
    #[cfg(not(feature = "test-support"))]
    assert_eq!(built, production);
}
