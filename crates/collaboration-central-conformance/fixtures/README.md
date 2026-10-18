# Access test key

`access-test-only-rsa.pk8` is a deliberately public, synthetic RSA fixture
created for local tests. It is never a production credential or key.
The native test client signs both Cloudflare Access application-token shapes;
the real native RSA and Worker WebCrypto implementations verify them against
an owned HTTP JWKS server. The Worker receives the original production-derived
certs URL; only the Miniflare harness redirects its outbound request.

The PKCS#8 DER file is the exact base64-decoded body of the previous PEM;
the key was not regenerated. Original PEM SHA256:
`3f32d47636cb3915cb7815e451332b9f1574ad70b6d485be914b19cdd304c990`.
DER SHA256:
`8a795709460f6100634df390437d7cc79eeaa7a8a674c19823e2da249f14157f`.
The client signs with ring 0.17.14; the native verifier uses ring's RS256
implementation. Native acceptance is restricted to 2048–4096-bit moduli,
preserving the previous upper bound and enforcing the released minimum.

`access-weak-1024.json` holds a separate public 1024-bit key and an otherwise
valid owner assertion, expiring in 2100. It is a permanent refusal fixture.
Its separate weak key was generated with OpenSSL (RSA, exponent 65537), used
once to sign those claims, and discarded; this did not replace the primary
fixture key. The old native verifier accepted that assertion in the recorded
RED. The new verifier must refuse it while the primary and tampered controls
continue to distinguish valid signatures from invalid ones.
