#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
#![allow(clippy::panic_in_result_fn)]
mod support;
use message_board::*;
use support::*;

#[tokio::test]
async fn historical_control_text_and_derived_subject_keep_the_shipped_visible_escape() -> TestResult
{
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    // The retained legacy test arranges this pre-write-boundary historical value.
    let source = rusqlite::Connection::open(&path)?;
    source.execute(
        "UPDATE board_messages SET text=? WHERE message_id='019a0000-0000-7000-8000-000000000065'",
        ["before\u{0007}after\nsecond line"],
    )?;
    source.close().map_err(|(_, error)| error)?;
    let bytes = std::fs::read(&path)?;
    let prepared = prepare_path(&path).await?;
    let project = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str().ends_with("0015"))
        .ok_or("project missing")?;
    let mut snapshot = project.store.snapshot().await?;
    let post_id = PostId::try_from("019a0000-0000-7000-8000-000000000065".to_owned())?;
    let Some(Record::Post(post)) = snapshot.record(&post_id.clone().into()).await? else {
        return Err("post missing".into());
    };
    assert_eq!(post.text.as_str(), "before\\u0007after\nsecond line");
    let Some(Record::Thread(thread)) = snapshot.record(&ThreadId::from(post_id).into()).await?
    else {
        return Err("thread missing".into());
    };
    let ThreadSubject::Project { line } = thread.about else {
        return Err("imported subject is not project-wide".into());
    };
    assert_eq!(line.as_str(), "before\\u0007after");
    snapshot.finish().await?;
    prepared.close().await;
    assert_eq!(bytes, std::fs::read(path)?);
    Ok(())
}

#[tokio::test]
async fn imported_subject_uses_first_line_unicode_characters_and_empty_line_fallback() -> TestResult
{
    for (text, expected) in [
        (
            "first line\nsecond line".to_owned(),
            "first line".to_owned(),
        ),
        ("λ".repeat(121), "λ".repeat(120)),
        (
            "\nbody".to_owned(),
            "Imported thread 019a0000-0000-7000-8000-000000000065".to_owned(),
        ),
    ] {
        let (_directory, path) = fixture_copy("role-history.sqlite")?;
        let source = rusqlite::Connection::open(&path)?;
        source.execute(
            "UPDATE board_messages SET text=? WHERE message_id='019a0000-0000-7000-8000-000000000065'",
            [&text],
        )?;
        source.close().map_err(|(_, error)| error)?;
        let prepared = prepare_path(&path).await?;
        let project = prepared
            .projects
            .iter()
            .find(|project| project.store.project_id().as_str().ends_with("0015"))
            .ok_or("project missing")?;
        let mut snapshot = project.store.snapshot().await?;
        let id = ThreadId::try_from("019a0000-0000-7000-8000-000000000065".to_owned())?;
        let Some(Record::Thread(thread)) = snapshot.record(&id.into()).await? else {
            return Err("thread missing".into());
        };
        let ThreadSubject::Project { line } = thread.about else {
            return Err("imported subject is not project-wide".into());
        };
        assert_eq!(line.as_str(), expected);
        snapshot.finish().await?;
        prepared.close().await;
    }
    Ok(())
}
