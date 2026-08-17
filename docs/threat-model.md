# Threat model: secrets at rest

What Komodo stores, what encrypting it buys, and — more importantly — what
it does not. The second half is the point: a feature named "encryption at
rest" invites people to assume protections it does not provide, and an
assumed protection is worse than a known gap.

## What is stored

Two kinds of credential live in Komodo's database:

| What | Where | Encrypted |
|---|---|---|
| `Variable.value` where `is_secret = true` | `variables` collection | yes |
| Git provider account tokens | `git_accounts` | yes |
| Image registry account tokens | `registry_accounts` | yes |
| `Variable.value` where `is_secret = false` | `variables` | no — by definition not a secret |
| API key secrets | `api_keys` | no — stored hashed, never recoverable |
| JWT / session material | not persisted | n/a |

Resource configs (Stack environments, Deployment env vars, Terraform
variables) are **not** encrypted. They are not meant to hold secrets:
Komodo's own answer for a secret inside a config is `[[VARIABLE]]`
interpolation, which resolves at execution time from the encrypted
`variables` collection. A secret pasted directly into a Stack's
environment is stored as typed. That is a documented limitation, not an
oversight — encrypting arbitrary config fields would mean guessing which
of them are secret, and guessing wrong in the direction of "not secret"
is silent.

## What this protects against

**A copy of the database without the host.** A dump handed to a vendor, a
backup on object storage, a read replica, a stolen disk, a mongo shell
opened by someone with network access but no shell on the Core host. In
every one of those the attacker now holds ciphertext and no key.

That is the whole claim. It is a real and common exposure — database
copies travel far more freely than hosts do — and it is the one this
buys.

## What this does NOT protect against

**Anyone who can read Core's config.** The key is in
`secret_keys` (or a file it points at). Config plus database is
equivalent to plaintext. This is not a flaw to be fixed later by moving
the key somewhere cleverer; it is inherent to Core needing to use these
secrets unattended, with no human to type a passphrase at boot.

**A compromised Core process.** Core decrypts on every read, so anything
that can run code in Core, read its memory, or call its API as an admin
gets plaintext. Encryption at rest is not a mitigation for application
compromise.

**Admins.** An admin can read secret Variables through the API by design —
that is what admin means here. The mask applied to non-admins is a UI
affordance, not a security boundary.

**Traffic.** Values are plaintext over the wire to Periphery and in
container environments. That is TLS's job and the container runtime's,
not this.

**Deletion.** Rotating a key does not re-encrypt old values, and nothing
here scrubs a secret from backups taken while it was plaintext. A
credential that has been exposed must be rotated at its source; changing
how Komodo stores it does not un-expose it.

## Design decisions and their consequences

**Opt-in, no migration.** No key configured means no encryption and no
behaviour change. Ciphertext carries a `komodo:enc:v<n>:` prefix, so a
value without one is plaintext and is returned as-is. Enabling encryption
on an existing instance is therefore not a flag day: old values keep
working and become encrypted the next time they are written.

The consequence, stated plainly: **enabling a key does not encrypt what
is already there.** An instance that has held a token for a year still
has that token in plaintext until someone updates it. If the reason for
turning this on is that a dump already leaked, rotate the credentials —
do not assume this reached backwards.

**The prefix is the only marker.** A `String` field holds plaintext and
ciphertext with equal validity; there is no type that makes an
un-encrypted write fail to compile. This is the weak point of the design.
It is why encryption lives at the small number of write sites rather than
being sprinkled at call sites, and why the read side decrypts at choke
points that every reader passes through.

**Keys are versioned by position, and the version travels with the
value.** Rotation is append-only: add a key, new writes use it, old
values still decrypt with the key that wrote them. Removing a key strands
every value it wrote — `decrypt` says exactly that when it happens rather
than returning garbage.

**XChaCha20-Poly1305.** AEAD, so tampering with a stored credential is
detected instead of silently decrypting to something else. The 192-bit
nonce is the reason for XChaCha specifically: it can be random for every
single value with no counter and no reuse risk. AES-GCM's 96-bit nonce
would require tracking one, and a repeat there leaks the key stream.

Every encryption of the same plaintext differs, so the database does not
reveal which secrets are equal to each other.

## Operating it

Generate a key:

```bash
openssl rand -base64 32
```

Point Core at it with `secret_keys = ["file:/config/keys/secret.key"]`,
or `KOMODO_SECRET_KEYS_FILE`. Prefer the file form: a key inline in the
config is one `cat` away in every log, backup, and screen share of that
file.

Rotate by appending:

```toml
secret_keys = [
  "file:/config/keys/secret.key",      # v1 - still needed to read old values
  "file:/config/keys/secret-2.key",    # v2 - new writes use this
]
```

**Keep the old keys.** They are not stale config; they are the only way
to read what they wrote.

Back up the keys separately from the database, and verify the backup by
restoring somewhere. A database backup whose keys were only ever on the
lost host is not a backup.

Verify it is actually on — read a secret straight out of the database and
look at it:

```bash
mongosh --eval 'db.variables.findOne({is_secret: true}).value'
# expected: komodo:enc:v1:AAAA...
# a bare secret here means encryption is not configured, or that value
# has not been written since it was turned on
```
