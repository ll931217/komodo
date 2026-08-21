# Extra CA certificates

Drop `.crt` files (PEM, one certificate per file) in this directory to
have the Docker builds trust them.

This exists for networks that intercept TLS: a corporate firewall
re-signs crates.io / registry.npmjs.org / jsr.io with its own CA, and
the base images only carry the public roots, so `cargo`, `yarn` and
`deno` fail with "self-signed certificate in certificate chain".

Two kinds of certificate live here:

- `vici-CA.crt` is **committed**. It is the internal root CA, and the images
  must trust it at RUNTIME - Core's own address, harbor, the doc host and
  gitlab all present certificates signed by it. Both shipped images assert it
  reached the system trust store, so a build that loses it fails loudly rather
  than producing an image that only breaks later at a handshake.
- Everything else is **gitignored** and host-sourced: `make docker-ca` copies
  the build host's `/usr/local/share/ca-certificates/*.crt` in, for the
  TLS-interception case described above. Those vary per host and per network,
  so they are deliberately not committed.
