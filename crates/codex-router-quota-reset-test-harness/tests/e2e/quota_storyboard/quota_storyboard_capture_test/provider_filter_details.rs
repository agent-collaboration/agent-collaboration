//! Literal real-component measured Details poses. This is the capture contract,
//! not a second rendering or fixture path. Offsets count Down inputs from Home,
//! in ascending order, following the compact Sessions Details poses documented
//! in the verification skill's tui-journeys.md. The full title lives
//! in the body's Original title section, below the fields.
pub(super) fn compact_session_details_poses(
    columns: u16,
    rows: u16,
) -> &'static [(&'static str, u16)] {
    match (columns, rows) {
        (32, 24) => &[
            ("compact-sessions-details", 0),
            ("compact-sessions-details-path", 5),
            ("compact-sessions-details-title", 8),
        ],
        (40, 24) => &[
            ("compact-sessions-details", 0),
            ("compact-sessions-details-path", 5),
            ("compact-sessions-details-title", 8),
        ],
        (48, 12) => &[
            ("compact-sessions-details-identity", 0),
            ("compact-sessions-details-state", 3),
            ("compact-sessions-details-path", 9),
            ("compact-sessions-details-title", 12),
        ],
        (48, 24) => &[
            ("compact-sessions-details", 0),
            ("compact-sessions-details-path", 1),
            ("compact-sessions-details-title", 4),
        ],
        (76, 30) => &[
            ("compact-sessions-details", 0),
            ("compact-sessions-details-path", 2),
            ("compact-sessions-details-title", 5),
        ],
        (120, 36) => &[
            ("compact-sessions-details", 0),
            ("compact-sessions-details-path", 0),
            ("compact-sessions-details-title", 0),
        ],
        _ => &[],
    }
}
pub(super) fn expected_session_checkpoints(columns: u16, rows: u16) -> Vec<&'static str> {
    let mut checkpoints = vec![
        "compact-sessions-all",
        "compact-sessions-codex-details",
        "compact-sessions-fork",
        "compact-sessions-fork-choice",
        "compact-sessions-new",
        "compact-sessions-new-machine",
        "compact-sessions-new-no-match",
        "compact-sessions-new-returned",
        "compact-sessions-machine-sources",
        "compact-sessions-client-no-match",
        "compact-sessions-chooser",
        "compact-sessions-help",
        "compact-sessions-claude",
        "compact-sessions-cancelled",
    ];
    checkpoints.extend(
        compact_session_details_poses(columns, rows)
            .iter()
            .map(|(name, _)| *name),
    );
    checkpoints.extend([
        "compact-sessions-folder-choice",
        "compact-sessions-model-provider-choice",
        "compact-sessions-source-choice",
        "compact-sessions-runtime-choice",
        "compact-sessions-sort-choice",
    ]);
    checkpoints
}
