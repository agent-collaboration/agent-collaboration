# Fleet Mail tests

Unit tests live beside their modules. They cover direct cancellation and late acknowledgement, expiry and its lack of sender effects, stored direct-message validation, and delivery deadlines.

The Tokio and Cloudflare host integration suites cover authorized sends, delivery, acknowledgements, alarm processing and drain replay through the composed application.
