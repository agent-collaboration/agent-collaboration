use crate::{
    oracle_errors::OracleError,
    oracle_hashes::sha256_post_set,
    post_selection::{SelectionMode, selected_post_ids},
    sealed_source::{
        LegacyActivity, LegacyActivityKind, LegacyMainStart, LegacySourceRows, LegacyThreadMark,
        LegacyThreadWatch, LegacyTopicMark,
    },
};

#[path = "oracle_io_tests.rs"]
mod oracle_io_tests;

fn mixed_history() -> LegacySourceRows {
    let activity = [
        (
            1,
            LegacyActivityKind::MainMessageCreated,
            Some("root"),
            None,
            "other",
        ),
        (2, LegacyActivityKind::Other, None, Some("root"), "reader"),
        (
            3,
            LegacyActivityKind::ThreadMessageCreated,
            Some("reply-before-main-mark"),
            Some("root"),
            "other",
        ),
        (
            4,
            LegacyActivityKind::MainMessageCreated,
            Some("main-before-mark"),
            None,
            "other",
        ),
        (
            6,
            LegacyActivityKind::ThreadMessageCreated,
            Some("reply-after-mark"),
            Some("root"),
            "other",
        ),
        (
            7,
            LegacyActivityKind::MainMessageCreated,
            Some("main-after-mark"),
            None,
            "other",
        ),
        (
            8,
            LegacyActivityKind::ThreadMessageCreated,
            Some("own-reply"),
            Some("root"),
            "reader",
        ),
    ]
    .into_iter()
    .map(
        |(sequence, kind, message_id, root_id, actor)| LegacyActivity {
            activity_sequence: sequence,
            project_id: "workspace".into(),
            topic_id: "project".into(),
            root_id: root_id.map(str::to_owned),
            kind,
            actor_key: actor.into(),
            message_id: message_id.map(str::to_owned),
        },
    )
    .collect();

    LegacySourceRows {
        activity,
        main_starts: vec![LegacyMainStart {
            reader_key: "reader".into(),
            project_id: "workspace".into(),
            main_start: Some(1),
        }],
        topic_marks: vec![LegacyTopicMark {
            reader_key: "reader".into(),
            topic_id: "project".into(),
            through_activity: 5,
        }],
        thread_marks: vec![LegacyThreadMark {
            reader_key: "reader".into(),
            root_id: "root".into(),
            through_activity: 2,
        }],
        thread_watches: vec![LegacyThreadWatch {
            reader_key: "reader".into(),
            root_id: "root".into(),
            starts_after_activity: 1,
            active: true,
        }],
        reader_keys: vec!["reader".into()],
        topic_ids: vec!["project".into()],
    }
}

fn post_ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn main_mark_sensitivity_rejects_a_project_wide_reply_cutoff() {
    let expected = post_ids(&[
        "reply-before-main-mark",
        "reply-after-mark",
        "main-after-mark",
    ]);

    assert_eq!(
        selected_post_ids(&mixed_history(), "reader", "project", SelectionMode::Unread,),
        expected,
    );
}

#[test]
fn latest_ignores_cutoffs_and_self_filter_and_orders_newest_first() {
    assert_eq!(
        selected_post_ids(&mixed_history(), "reader", "project", SelectionMode::Latest,),
        post_ids(&[
            "own-reply",
            "main-after-mark",
            "reply-after-mark",
            "main-before-mark",
            "reply-before-main-mark",
            "root",
        ]),
    );
}

#[test]
fn thread_mark_filters_replies_without_filtering_main_posts() {
    let mut source = mixed_history();
    source.thread_marks = vec![LegacyThreadMark {
        reader_key: "reader".into(),
        root_id: "root".into(),
        through_activity: 6,
    }];

    assert_eq!(
        selected_post_ids(&source, "reader", "project", SelectionMode::Unread),
        post_ids(&["main-after-mark"]),
    );
}

#[test]
fn inactive_watch_excludes_replies_in_both_modes() {
    let mut source = mixed_history();
    source.thread_watches = vec![LegacyThreadWatch {
        reader_key: "reader".into(),
        root_id: "root".into(),
        starts_after_activity: 1,
        active: false,
    }];

    assert_eq!(
        selected_post_ids(&source, "reader", "project", SelectionMode::Unread),
        post_ids(&["main-after-mark"]),
    );
    assert_eq!(
        selected_post_ids(&source, "reader", "project", SelectionMode::Latest),
        post_ids(&["main-after-mark", "main-before-mark", "root"]),
    );
}

#[test]
fn missing_main_start_still_allows_watched_replies() {
    let mut source = mixed_history();
    source.main_starts.clear();

    assert_eq!(
        selected_post_ids(&source, "reader", "project", SelectionMode::Unread),
        post_ids(&["reply-before-main-mark", "reply-after-mark"]),
    );
}

#[test]
fn watch_start_is_exclusive() {
    let mut source = mixed_history();
    source.thread_watches = vec![LegacyThreadWatch {
        reader_key: "reader".into(),
        root_id: "root".into(),
        starts_after_activity: 3,
        active: true,
    }];

    assert_eq!(
        selected_post_ids(&source, "reader", "project", SelectionMode::Unread),
        post_ids(&["reply-after-mark", "main-after-mark"]),
    );
}

#[test]
fn covered_post_set_digest_sorts_and_deduplicates_ids() -> Result<(), OracleError> {
    let first = vec!["post-b".to_owned(), "post-a".to_owned()];
    let reordered_with_duplicate = vec![
        "post-a".to_owned(),
        "post-b".to_owned(),
        "post-a".to_owned(),
    ];

    assert_eq!(
        sha256_post_set(&first)?,
        sha256_post_set(&reordered_with_duplicate)?,
    );
    assert_eq!(
        sha256_post_set(&first)?,
        "7b2489aa0e27a435373c95235faa03a508411911b2881c92d84a751254b1bfc5",
    );
    Ok(())
}
