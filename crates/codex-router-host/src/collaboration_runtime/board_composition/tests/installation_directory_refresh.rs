//! Literal central pages cross the real pump, event owner and installation store.
use super::*;
use board_sync_protocol::InstallationPeerPage;
use fleet_sync_protocol::PeerId;
use message_board::ServiceId;

#[tokio::test]
async fn welcome_walks_every_page_and_connected_timer_learns_a_new_installation()
-> Result<(), TestError> {
    let mut harness = MailHarness::open(false).await?;
    let first = binding(1)?;
    let second = binding(2)?;
    let later = binding(3)?;
    harness
        .central
        .directory
        .answer_page(
            None,
            InstallationPeerPage::new(vec![first.clone()], Some(first.1))?,
        )
        .await;
    harness
        .central
        .directory
        .answer_page(
            Some(first.1),
            InstallationPeerPage::new(vec![second.clone()], None)?,
        )
        .await;
    tokio::time::pause();
    // Keep paused native I/O from advancing the timer before explicit advance.
    let clock_guard = tokio::spawn(async {
        loop {
            tokio::task::yield_now().await;
        }
    });
    let result = async {
        if harness
            .mail
            .owner
            .installation_peer(&first.0)
            .await?
            .is_some()
        {
            return Err("directory refreshed before Host Welcome".into());
        }
        harness.release_welcome()?;
        wait_binding(&harness, &first).await?;
        wait_binding(&harness, &second).await?;
        harness
            .central
            .directory
            .answer_page(None, InstallationPeerPage::new(vec![later.clone()], None)?)
            .await;
        tokio::time::advance(Duration::from_secs(60)).await;
        wait_binding(&harness, &later).await?;
        // Refreshes append immutable bindings; a later page does not erase older peers.
        wait_binding(&harness, &first).await?;
        wait_binding(&harness, &second).await?;
        Ok::<_, TestError>(())
    }
    .await;
    tokio::time::resume();
    clock_guard.abort();
    let _ = clock_guard.await;
    let closed = harness.close().await;
    match (result, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(close)) => Err(format!("scenario: {error}; cleanup: {close}").into()),
    }
}

fn binding(number: u128) -> Result<(ServiceId, PeerId), TestError> {
    Ok((
        uuid::Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0100 + number)
            .to_string()
            .try_into()?,
        PeerId::new(uuid::Uuid::from_u128(
            0x0000_0000_0000_7000_8000_0000_0000_0000 + number,
        ))?,
    ))
}

async fn wait_binding(
    harness: &MailHarness,
    binding: &(ServiceId, PeerId),
) -> Result<(), TestError> {
    let bound = std::time::Instant::now() + BOUND;
    loop {
        if let Some(peer) = harness.mail.owner.installation_peer(&binding.0).await?
            && peer.as_str() == binding.1.as_uuid().to_string()
        {
            return Ok(());
        }
        if std::time::Instant::now() >= bound {
            return Err(format!(
                "installation binding {} was not persisted",
                binding.0.as_str()
            )
            .into());
        }
        tokio::task::yield_now().await;
    }
}
