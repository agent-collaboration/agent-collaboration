//! Recording provider fixture with explicit response release, never a sleep.
use super::*;

#[derive(Clone)]
pub(super) struct RecordingRefreshClient {
    expected_account_id: String,
    expected_refresh_token: String,
    response: AccountCredentialBundle,
    calls: Arc<AtomicUsize>,
    response_gate: Option<Arc<RefreshResponseGate>>,
}

impl RecordingRefreshClient {
    pub(super) fn new(response: AccountCredentialBundle) -> Self {
        Self::new_for_account("acct_refresh_bundle", "refresh-token-canary", response)
    }

    pub(super) fn new_for_account(
        expected_account_id: &str,
        expected_refresh_token: &str,
        response: AccountCredentialBundle,
    ) -> Self {
        Self {
            expected_account_id: expected_account_id.to_owned(),
            expected_refresh_token: expected_refresh_token.to_owned(),
            response,
            calls: Arc::new(AtomicUsize::new(0)),
            response_gate: None,
        }
    }

    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub(super) fn with_response_gate(
        mut self,
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> Self {
        self.response_gate = Some(Arc::new(RefreshResponseGate {
            entered,
            release: Mutex::new(release),
        }));
        self
    }
}

impl CredentialRefreshClient for RecordingRefreshClient {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    fn refresh_credentials(
        &self,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, crate::resolver::CredentialRefreshFailure> {
        assert_eq!(account_id.as_str(), self.expected_account_id);
        assert_eq!(refresh_token.expose_secret(), self.expected_refresh_token);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.response_gate {
            gate.wait_for_release();
        }
        Ok(self.response.clone())
    }
}

struct RefreshResponseGate {
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl RefreshResponseGate {
    fn wait_for_release(&self) {
        self.entered
            .send(())
            .expect("provider entry must be observed");
        self.release
            .lock()
            .expect("provider release lock")
            .recv_timeout(Duration::from_secs(5))
            .expect("test must release provider response");
    }
}
