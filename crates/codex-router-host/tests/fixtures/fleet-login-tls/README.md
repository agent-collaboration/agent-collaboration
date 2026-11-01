These are public, disposable TLS fixtures for F11's isolated HTTPS issuer.
The server certificate covers only localhost and 127.0.0.1 and is signed by
this fixture root. The checked-in private key is deliberately public and
must never be used for a deployment. Only test-injected reqwest clients
trust root.der; production trust and TLS validation are unchanged.

The certificates were generated once outside the test. Tests load fixed
DER bytes and never generate certificates, read a clock or disable certificate
verification. The leaf's fixed validity is 2020–2126. Both TLS configurations
use a fixed TimeProvider; neither the fixture nor certificate validation reads
the wall clock. Socket accept, handshake and shutdown wait only on I/O events.
