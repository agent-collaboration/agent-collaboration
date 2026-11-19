//! The driver's side of the machine token seam: connect only with a Ready
//! token, stop retrying on NeedsNewCode until the source changes, renew the
//! open connection when the source supplies a new token, and report every
//! renewal result back to the source.
use super::super::link_transport::TokenAvailability;
use super::LinkDriver;
use central_link_client::{LinkStatus, RefusalReason};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, MachineTokenText, TokenRenewalRefusal, TokenRenewalResult,
};
use std::time::Duration;

/// How long after a CentralUnavailable renewal answer the driver tries again.
const RENEWAL_RETRY: Duration = Duration::from_secs(5);

impl LinkDriver {
    /// The source changed state: a Ready token after a NeedsNewCode refusal
    /// starts connecting again; a new Ready token on an open connection is
    /// sent as RenewMachineToken. Other states wait for the next change.
    pub(super) async fn token_changed(&mut self) {
        let TokenAvailability::Ready(token) = self.tokens.availability() else {
            return;
        };
        match self.client.status() {
            LinkStatus::Refused { .. } => self.client.reenrolled(),
            LinkStatus::Connected { .. } => self.renew_with(token).await,
            LinkStatus::Connecting => {}
        }
    }

    async fn renew_with(&mut self, token: MachineTokenText) {
        self.renew_retry_at = None;
        let already_sent = self.connected_token.as_ref() == Some(&token)
            || self
                .renewing
                .as_ref()
                .is_some_and(|(_, renewing)| renewing == &token);
        if already_sent {
            return;
        }
        let correlation_id = self.renewal_correlation();
        let frame = LinkFrame::Data(DataFrame::RenewMachineToken {
            correlation_id: correlation_id.clone(),
            token: token.clone(),
        });
        if self.send(&frame).await {
            self.renewing = Some((correlation_id, token));
        } else {
            self.socket_closed().await;
        }
    }

    /// Central answered a RenewMachineToken. The source hears every result;
    /// a renewed connection now runs on the token it carried.
    pub(super) fn renewal_answered(
        &mut self,
        correlation_id: &CorrelationId,
        result: &TokenRenewalResult,
    ) {
        let Some((sent, token)) = self.renewing.take() else {
            return;
        };
        if &sent != correlation_id {
            self.renewing = Some((sent, token));
            return;
        }
        match result {
            TokenRenewalResult::Renewed { .. } => self.connected_token = Some(token),
            // Transient: the source keeps Ready, so no change will arrive; the
            // driver keeps the link and sends the renewal again shortly.
            TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::CentralUnavailable,
            } => {
                self.renew_retry_at = Some(self.clock.monotonic_now().after(RENEWAL_RETRY));
            }
            TokenRenewalResult::Refused { .. } => {}
        }
        self.tokens.report_renewal(result);
    }

    /// Retries a renewal central could not answer, once its delay has passed,
    /// if the link is still open and the source still offers a newer token.
    pub(super) async fn drive_renewal_retry(&mut self) {
        if !self
            .renew_retry_at
            .is_some_and(|at| self.clock.monotonic_now() >= at)
        {
            return;
        }
        self.renew_retry_at = None;
        if !matches!(self.client.status(), LinkStatus::Connected { .. }) {
            return;
        }
        if let TokenAvailability::Ready(token) = self.tokens.availability() {
            self.renew_with(token).await;
        }
    }

    /// Refused by a NeedsNewCode source: "not enrolled; new code needed".
    pub(super) fn not_enrolled(&self) -> bool {
        matches!(
            self.client.status(),
            LinkStatus::Refused {
                reason: RefusalReason::CredentialRefused
            }
        )
    }

    fn renewal_correlation(&mut self) -> CorrelationId {
        let millis = u64::try_from(self.clock.utc_now().timestamp_millis()).unwrap_or(0);
        let nanos = u32::try_from(millis % 1000)
            .unwrap_or(0)
            .saturating_mul(1_000_000);
        CorrelationId::mint(uuid::Timestamp::from_unix(
            &self.renewal_ids,
            millis / 1000,
            nanos,
        ))
    }
}

#[cfg(test)]
#[path = "driver_tokens_test.rs"]
mod tests;
