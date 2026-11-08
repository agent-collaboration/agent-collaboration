//! Races forced by a barrier: a provisioning call held after its statement
//! verifies and before its applying transaction, while another commits. The
//! held call's transaction must decide on what is stored then, not on what it
//! saw when it verified.
#![cfg(feature = "conformance")]
use crate::organization_provisioning_link::{
    ORGANIZATION_A, PROVISION, control, provision_body, start, stored,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, SignInKeys, link_post};
use collaboration_central_conformance::{
    ConformanceResult, DispatchCallSelector, DispatchKey, JwksFixtureServer, RuntimeControl,
};
use collaboration_central_core::ObjectAddress;

pub(crate) fn held_provisioning() -> ConformanceResult<DispatchKey> {
    Ok(DispatchKey::new(
        ObjectAddress::Fleet {
            organization: ORGANIZATION_A.to_owned().try_into()?,
        },
        DispatchCallSelector::OrganizationProvisioning,
    ))
}

/// Hold `held`'s provisioning after it verifies, commit `racing`, then release.
async fn race(
    base: &str,
    socket: &std::path::Path,
    held: String,
    racing: String,
) -> ConformanceResult<((u16, Vec<u8>), (u16, Vec<u8>))> {
    let key = held_provisioning()?;
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::HoldNextDispatch { key: key.clone() },
    )
    .await?;
    let held_socket = socket.to_owned();
    let held_call = tokio::spawn(async move {
        link_post(
            &held_socket,
            PROVISION,
            &provision_body(ORGANIZATION_A, &held),
        )
        .await
        .map_err(|error| error.to_string())
    });
    // Returns once the held call has verified and stopped before applying.
    control(
        base,
        ORGANIZATION_A,
        RuntimeControl::AwaitHeldDispatch { key: key.clone() },
    )
    .await?;
    let racing_answer =
        link_post(socket, PROVISION, &provision_body(ORGANIZATION_A, &racing)).await?;
    control(base, ORGANIZATION_A, RuntimeControl::ResumeDispatch { key }).await?;
    let held_answer = held_call.await??;
    Ok((held_answer, racing_answer))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare both answers and the cold-read store while fixture errors propagate."
)]
#[tokio::test]
async fn two_first_provisions_at_once_end_with_one_stamp() -> ConformanceResult<()> {
    // Wrong build caught: a held first provision that applies on what it saw
    // when it verified (no stamp), writing a second identity or record, or
    // replacing the stamp that committed in between. A setup statement with
    // no predecessor over a stored stamp answers Provisioned and changes
    // nothing, whatever its bytes.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    let held = stamps::stamp(ORGANIZATION_A)?;
    let racing = stamps::first_statement(ORGANIZATION_A, 1)?;
    let (held_answer, racing_answer) =
        race(&base, &central.socket, held.clone(), racing.clone()).await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(racing_answer.0, 200, "the racing provision commits");
    assert_eq!(
        held_answer.0, 200,
        "the held setup finds a stamp and answers Provisioned, changing nothing"
    );
    assert_eq!(
        identity.iter().map(|row| row.1.clone()).collect::<Vec<_>>(),
        vec![racing],
        "one stamp: the one that committed first"
    );
    assert_eq!(records.len(), 1, "one setup record");
    assert_ne!(
        held,
        identity
            .first()
            .map(|row| row.1.clone())
            .unwrap_or_default()
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare both answers and the cold-read store while fixture errors propagate."
)]
#[tokio::test]
async fn two_identical_first_provisions_at_once_answer_provisioned_with_one_record()
-> ConformanceResult<()> {
    // Wrong build caught: a held byte-identical setup that writes a second
    // identity or record, or refuses a replay of what is stored.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    let statement = stamps::stamp(ORGANIZATION_A)?;
    let (held_answer, racing_answer) =
        race(&base, &central.socket, statement.clone(), statement.clone()).await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!((held_answer.0, racing_answer.0), (200, 200));
    assert_eq!(
        identity.iter().map(|row| row.1.clone()).collect::<Vec<_>>(),
        vec![statement]
    );
    assert_eq!(records.len(), 1, "one setup record");
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare both answers and the cold-read store while fixture errors propagate."
)]
#[tokio::test]
async fn two_renewals_of_one_stamp_at_once_let_only_one_win() -> ConformanceResult<()> {
    // Wrong build caught: a held renewal whose predecessor check used the
    // stamp it saw when it verified, so it overwrites the renewal that
    // committed in between.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let central = start(root.path(), &access, &keys, NOW_SECONDS).await?;
    let base = central.server.base_url().to_owned();
    let original = stamps::stamp(ORGANIZATION_A)?;
    let provisioned = link_post(
        &central.socket,
        PROVISION,
        &provision_body(ORGANIZATION_A, &original),
    )
    .await?;
    let held = stamps::renewal(ORGANIZATION_A, &original, 10)?;
    let racing = stamps::renewal(ORGANIZATION_A, &original, 11)?;
    let (held_answer, racing_answer) = race(&base, &central.socket, held, racing.clone()).await?;
    central.server.shutdown().await?;
    let (identity, records) = stored(root.path(), ORGANIZATION_A).await?;

    assert_eq!(provisioned.0, 200);
    assert_eq!(racing_answer.0, 200, "the racing renewal wins");
    assert_eq!(
        held_answer.0, 409,
        "the held renewal names a predecessor that is no longer stored"
    );
    assert_eq!(
        identity.iter().map(|row| row.1.clone()).collect::<Vec<_>>(),
        vec![racing],
        "the winning renewal is the stored stamp"
    );
    assert_eq!(records.len(), 2, "setup + one renewal record");
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
