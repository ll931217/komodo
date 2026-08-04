# Extra CA certificates

Drop `.crt` files (PEM, one certificate per file) in this directory to
have the Docker builds trust them.

This exists for networks that intercept TLS: a corporate firewall
re-signs crates.io / registry.npmjs.org / jsr.io with its own CA, and
the base images only carry the public roots, so `cargo`, `yarn` and
`deno` fail with "self-signed certificate in certificate chain".

The directory ships empty, so the builds are unchanged without it.
Certificates placed here are gitignored.
