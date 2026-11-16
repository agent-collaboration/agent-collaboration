import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-secret-store",
  files: [
    {
      file: "crates/codex-router-secret-store/src/account_credential_lock.rs",
      disposition: { kind: "none" },
      note: "Cross-process per-account file lock beside the state database (AccountCredentialLock::acquire L35-L51, lock directory L23-L27, SHA-256 of the account id as file name L45-L46); no serde, no message shape of the four protocols. In-file #[cfg(all(test, unix))] module L92.",
    },
    {
      file: "crates/codex-router-secret-store/src/account_tokens.rs",
      disposition: { kind: "none" },
      note: "Private secret-store payload for one OpenAI account credential: AccountCredentialBundlePayload (Serialize L168-L176) / OwnedAccountCredentialBundlePayload (Deserialize L178-L186) encoded by to_secret_string L97-L114 and decoded by from_secret_string L117-L149; access_token_exp_unix_seconds L254-L258 base64-decodes the OpenAI OAuth access-token JWT payload and reads its exp claim. Private storage format and OAuth/JWT, neither one of the four protocols nor carried on any of the six edges. Secret keys openai_access_token./openai_refresh_token. L189-L196.",
    },
    {
      file: "crates/codex-router-secret-store/src/affinity_secret.rs",
      disposition: { kind: "none" },
      note: "Loads or creates the Router affinity HMAC secret under key router_affinity_hash_secret.v1 (L15, L59-L78) as raw lowercase hex from /dev/urandom (L98-L141); no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/backend.rs",
      disposition: { kind: "none" },
      note: "SecretStore trait (L9-L37): write/read/stage/delete/prune of SecretString values by SecretKey; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/central_credentials.rs",
      disposition: { kind: "none" },
      note: "CentralCredential (Serialize+Deserialize, tag kind, L40-L61: Owner/Enrollment with endpoint, assertion, expires_at, client_id) is the private encrypted-storage and one-shot 0600 handoff-file encoding of central sign-in assertions (encode_secret/decode_secret L64-L75, write_private_file L76-L88, consume_private_file L90-L133); CentralSecretKind key names L12-L36. Central sign-in credential storage between Router and central, not one of the four protocols or six edges; no SessionRef or other Router-vocabulary identity is converted.",
    },
    {
      file: "crates/codex-router-secret-store/src/credential_bundle.rs",
      disposition: { kind: "none" },
      note: "CredentialBundle enum (L17-L29) over OpenAI and Claude OAuth tokens; ClaudeCredentialBundlePayload (Serialize L198-L206) / OwnedClaudeCredentialBundlePayload (Deserialize L208-L216) are the private secret-store payload (to_secret_string L113-L143, from_secret_string L146-L172). Private storage of provider OAuth tokens, not one of the four protocols. In-file #[cfg(test)] module L230.",
    },
    {
      file: "crates/codex-router-secret-store/src/credential_key.rs",
      disposition: { kind: "none" },
      note: "AccountCredentialKey (L41-L132) parses and renders <provider>_credential_bundle.<account>.<generation> secret-store file keys; classify_secret_key L18-L28 picks the storage class. Private storage key naming; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/credential_migration.rs",
      disposition: { kind: "none" },
      note: "Startup migration of legacy .secret credential files to encrypted .v2 envelopes under the exclusive store lock (migrate_pooled_credentials_at_startup L39-L116, migrate_with_exclusive_lock L141-L322, read-back verification L324-L337); CredentialMigrationOutcome L24-L36 is an in-process result. Private storage only; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/credential_store_lock.rs",
      disposition: { kind: "none" },
      note: "Shared/exclusive .store.lock file lock for the pooled credential store (CredentialStoreLock::acquire L29-L54); no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/encrypted_credential_store.rs",
      disposition: { kind: "none" },
      note: "AES-256-GCM pooled credential store: EncryptedCredentialEnvelope (Serialize+Deserialize, deny_unknown_fields, L473-L479: format, nonce, ciphertext) written by encrypt_envelope L393-L425 and read by decrypt_envelope L427-L471; store state and status L33-L60, L262-L273. Private encrypted storage format, not one of the four protocols or six edges.",
    },
    {
      file: "crates/codex-router-secret-store/src/file_backend.rs",
      disposition: { kind: "none" },
      note: 'FileSecretStore (L31-L39, open L185-L201, open_read_only L226-L250) writes <key>.secret files atomically through private temps (write_atomic_file L456-L482), plus store-id and format-v2.marker metadata (L325-L350); test-support read/write traces L56-L181 behind cfg(any(test, feature = "test-support")). Private filesystem storage; no message shape of the four protocols.',
    },
    {
      file: "crates/codex-router-secret-store/src/file_backend/debug_plaintext.rs",
      disposition: { kind: "none" },
      note: "Debug-build plaintext pooled-credential policy: declaration marker debug-plaintext.marker L20-L21 (read_declaration L191-L202), root validation L204-L327, guarded plaintext delete/prune L24-L188. Private storage policy; no message shape of the four protocols. #[cfg(test)] module declared L329-L331.",
    },
    {
      file: "crates/codex-router-secret-store/src/file_backend/pooled_credential_files.rs",
      disposition: { kind: "none" },
      note: "Encrypted <key>.v2 envelope file read/write/delete (L22-L64), migration-only legacy access (L66-L91) and directory scans for credential, temp and account-name entries (L93-L355). Private filesystem storage; no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/fleet_secret_key.rs",
      disposition: { kind: "none" },
      note: "FleetSecretKey (L21-L82) parses and renders fleet_<kind>.<installation uuid>.<generation uuid> secret-store keys with canonical UUID checks L84-L90. Private storage key naming for central fleet secrets, not one of the four protocols; installation/generation are fleet enrollment ids, not Router-vocabulary identities.",
    },
    {
      file: "crates/codex-router-secret-store/src/keychain_data_key.rs",
      disposition: { kind: "none" },
      note: 'Router-owned macOS Keychain item holding the pooled-credential AES key: KeychainAccess trait L51-L66, platform adapter L80-L187, load_or_create_pooled_credential_data_key L207-L240, store-id UUIDv7 L291-L311. OS Keychain storage; no message shape of the four protocols. #[cfg(all(test, target_os = "macos"))] module L313-L315 and #[cfg(test)] module L317.',
    },
    {
      file: "crates/codex-router-secret-store/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module declarations L3-L23 and L566 (account_credential_lock), SecretStore re-export L34, package_name L37-L40; #[cfg(test)] modules L6-L7, L25-L32 and L42. No message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/local_router_token.rs",
      disposition: { kind: "none" },
      note: "LocalRouterTokenService (L53-L197) stores the local Router bearer token and its generation as plain secret-store strings (keys L261-L275, generation parse L250-L259, random hex token L277-L291) under a .token.lock creation lock L199-L248. Private token storage; no message shape of the four protocols. #[cfg(test)] module L293-L295.",
    },
    {
      file: "crates/codex-router-secret-store/src/model.rs",
      disposition: { kind: "none" },
      note: "SecretKey file-safe key (L9-L27), SecretStoreError (L31-L183), CredentialMigrationFailure (L187-L215) and StoreUnavailable (L219-L228) in-process error/model types; no serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/refresh_lease.rs",
      disposition: { kind: "none" },
      note: "In-memory RefreshLeaseManager (L65-L129) with injected LeaseClock (L8-L39); LeaseAcquisition L43-L53. No serde, no message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/runtime_credential_store.rs",
      disposition: { kind: "none" },
      note: "RuntimeCredentialStore (L14-L20) dispatches SecretStore calls to the encrypted or debug-plaintext backend (L76-L121) and maps backend status (L59-L73). No message shape of the four protocols.",
    },
    {
      file: "crates/codex-router-secret-store/src/test_support.rs",
      disposition: { kind: "none" },
      note: 'Test-support module declared at lib.rs:22-23 under cfg(any(test, feature = "test-support")) (not cfg(test)-only): deterministic fixture Keychain DeterministicTestKeychainAccess L22-L47 and fixed-key encrypted store openers L50-L115 with a production-root guard L118-L194. No message shape of the four protocols. In-file #[cfg(test)] module L196.',
    },
  ],
  sendSites: [],
};
