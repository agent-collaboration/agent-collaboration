//! A failed owner pass, and a failed held-push clean-up, retry on a 1 s to 30 s backoff
//! and log once per run of failures.

// SQLite works on background threads; the owner's retries run on the injected clock only.
