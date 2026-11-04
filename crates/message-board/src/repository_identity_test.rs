use super::CommonDirectory;

#[test]
fn common_directory_requires_a_rooted_unix_path_without_changing_its_bytes() {
    for rooted in [
        "/",
        "/repository/shared/λ",
        "/repository/../shared",
        "//repository/shared",
    ] {
        let directory = CommonDirectory::try_from(rooted.to_owned()).unwrap();
        assert_eq!(directory.as_str(), rooted);
    }
    for invalid in [
        "",
        "repository/shared",
        "./repository",
        "../repository",
        "C:\\repository",
        "/repository\0/shared",
    ] {
        assert!(
            CommonDirectory::try_from(invalid.to_owned()).is_err(),
            "{invalid:?}"
        );
    }
    assert!(CommonDirectory::try_from(format!("/{}", "a".repeat(4096))).is_err());
}
