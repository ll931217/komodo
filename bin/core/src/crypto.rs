//! Encryption at rest for the secrets Komodo stores in its database.
//!
//! Komodo keeps two kinds of credential in MongoDB: the value of a
//! Variable flagged `is_secret`, and the tokens on git provider and
//! image registry accounts. Both were plaintext, so a database dump -
//! or a backup, or a read replica, or anyone with a mongo shell - was
//! every credential the instance holds.
//!
//! ## Why application-level
//!
//! MongoDB's own field-level encryption is not available: prod runs
//! MongoDB but the e2e harness runs FerretDB, which does not implement
//! CSFLE. Anything that has to work on both has to be done here,
//! before the value reaches the driver.
//!
//! ## The envelope
//!
//! Ciphertext is stored in the same `String` field the plaintext used,
//! because those structs are simultaneously the database document and
//! the HTTP DTO - there is no separate storage type to widen. The
//! stored form is:
//!
//! ```text
//! komodo:enc:v1:<base64(nonce || ciphertext || tag)>
//! ```
//!
//! Two consequences, both deliberate:
//!
//! - **A value without the prefix is plaintext**, and [decrypt]
//!   returns it unchanged. That is what makes this deployable against
//!   an existing database with no migration: rows written before this
//!   change keep working, and each is encrypted the next time it is
//!   written. The alternative - a migration that rewrites every secret
//!   in place - is a single irreversible pass over exactly the data
//!   you cannot afford to corrupt.
//! - **The version travels with the value**, so keys can be rotated by
//!   adding a new one without re-encrypting anything. Old keys stay in
//!   the config and stay able to decrypt what they wrote.
//!
//! ## What this does not protect against
//!
//! The key lives in Core's config, so anyone who can read the config
//! can decrypt the database. This raises the cost of a stolen database
//! from "you have the secrets" to "you also need the host", which is
//! the threat this bead names. It is not protection against a
//! compromised Core process. See `docs/threat-model.md`.

use anyhow::{Context, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chacha20poly1305::{
  KeyInit, XChaCha20Poly1305, XNonce,
  aead::{Aead, OsRng, rand_core::RngCore},
};

use crate::config::core_config;

/// Marks a value as encrypted by this module, and carries the key
/// version that wrote it.
const PREFIX: &str = "komodo:enc:v";

/// XChaCha20-Poly1305 nonce width. Chosen over AES-GCM's 96 bits
/// precisely because 192 bits is wide enough to generate at random for
/// every value without tracking a counter to avoid reuse - and a
/// reused nonce in a GCM-family cipher leaks the key stream.
const NONCE_BYTES: usize = 24;

/// Whether `value` is one of ours.
///
/// A bare `String` accepts plaintext and ciphertext with equal
/// validity, so this is the only thing standing between "encrypted"
/// and "we thought it was encrypted".
pub fn is_encrypted(value: &str) -> bool {
  parse(value).is_some()
}

/// The key version that wrote `value`, or None if it is plaintext.
///
/// Exists for the re-encrypt pass, which must tell "already written
/// by the newest key" (skip) from "written by an older key, or not
/// encrypted at all" (rewrite). Without it that pass would decrypt
/// and re-encrypt every secret on every run, churning the database
/// and generating a new nonce for values that were already current.
pub fn key_version(value: &str) -> Option<u32> {
  parse(value).map(|(version, _)| version)
}

/// The version new writes use, or None when encryption is off.
pub fn newest_version() -> anyhow::Result<Option<u32>> {
  Ok(newest_key()?.map(|(version, _)| version))
}

/// `(version, payload)` for an encrypted value, or None for plaintext.
fn parse(value: &str) -> Option<(u32, &str)> {
  let rest = value.strip_prefix(PREFIX)?;
  let (version, payload) = rest.split_once(':')?;
  Some((version.parse().ok()?, payload))
}

/// Encrypt with the newest configured key.
///
/// A no-op when no key is configured: encryption at rest is opt-in, so
/// an instance that has not set one keeps working exactly as before
/// rather than failing every write.
/// Encrypt, but leave an empty value empty.
///
/// Encrypting "" produces non-empty ciphertext that decrypts back to "",
/// which turns "this account has no ssh key" into "has one, and it is
/// blank" - and the consumer then reports a half-configured credential
/// for an account that simply never had one. Absence has to survive the
/// round trip.
pub fn encrypt_if_set(plaintext: &str) -> anyhow::Result<String> {
  if plaintext.is_empty() {
    return Ok(String::new());
  }
  encrypt(plaintext)
}

pub fn encrypt(plaintext: &str) -> anyhow::Result<String> {
  let Some((version, key)) = newest_key()? else {
    return Ok(plaintext.to_string());
  };

  let cipher = XChaCha20Poly1305::new(&key.into());
  let mut nonce = [0u8; NONCE_BYTES];
  OsRng
    .try_fill_bytes(&mut nonce)
    .context("Failed to generate a nonce")?;
  let nonce = XNonce::from_slice(&nonce);

  let ciphertext = cipher
    .encrypt(nonce, plaintext.as_bytes())
    .map_err(|e| anyhow!("Failed to encrypt: {e}"))?;

  let mut payload = nonce.to_vec();
  payload.extend_from_slice(&ciphertext);

  Ok(format!("{PREFIX}{version}:{}", BASE64.encode(payload)))
}

/// Decrypt a value written by [encrypt].
///
/// Plaintext passes through untouched - that is how rows written
/// before encryption was enabled keep working.
pub fn decrypt(value: &str) -> anyhow::Result<String> {
  let Some((version, payload)) = parse(value) else {
    return Ok(value.to_string());
  };

  let key = key_for_version(version)?.with_context(|| {
    format!(
      "No key configured for encrypted value version {version}. \
       The key that wrote it must stay in 'secret_keys' - removing a \
       key does not rotate anything, it strands every value it wrote."
    )
  })?;

  let payload = BASE64
    .decode(payload)
    .context("Encrypted value is not valid base64")?;
  if payload.len() <= NONCE_BYTES {
    return Err(anyhow!("Encrypted value is truncated"));
  }
  let (nonce, ciphertext) = payload.split_at(NONCE_BYTES);

  let cipher = XChaCha20Poly1305::new(&key.into());
  let plaintext = cipher
    .decrypt(XNonce::from_slice(nonce), ciphertext)
    .map_err(|_| {
      anyhow!(
        "Failed to decrypt a stored secret with key version {version}. \
         Either the key changed or the value was altered."
      )
    })?;

  String::from_utf8(plaintext)
    .context("Decrypted secret is not valid UTF-8")
}

/// Encrypt only when the caller says this value is secret.
///
/// Exists so call sites read as the rule they are implementing -
/// `maybe_encrypt(value, is_secret)` - rather than repeating an `if`
/// that is easy to get backwards at one of the sites and nowhere else.
pub fn maybe_encrypt(
  value: &str,
  is_secret: bool,
) -> anyhow::Result<String> {
  if is_secret {
    encrypt(value)
  } else {
    Ok(value.to_string())
  }
}

/// The configured keys, newest last, as raw 32-byte keys.
///
/// Keys are configured base64-encoded; `file:` paths are resolved by
/// the config loader before this sees them, the same as every other
/// secret in the core config.
fn keys() -> anyhow::Result<Vec<[u8; 32]>> {
  core_config()
    .secret_keys
    .iter()
    .enumerate()
    .map(|(i, key)| {
      let bytes = BASE64.decode(key).with_context(|| {
        format!("secret_keys[{i}] is not valid base64")
      })?;
      let len = bytes.len();
      bytes.try_into().map_err(|_| {
        anyhow!(
          "secret_keys[{i}] must decode to exactly 32 bytes, got {len}"
        )
      })
    })
    .collect()
}

/// `(version, key)` for the newest configured key, or None when
/// encryption is not configured.
///
/// Version is 1-based and is the key's position, so appending a key
/// gives it a new version and leaves every existing value decryptable.
fn newest_key() -> anyhow::Result<Option<(u32, [u8; 32])>> {
  let keys = keys()?;
  Ok(keys.last().map(|key| (keys.len() as u32, *key)))
}

fn key_for_version(version: u32) -> anyhow::Result<Option<[u8; 32]>> {
  let keys = keys()?;
  Ok(
    version
      .checked_sub(1)
      .and_then(|index| keys.get(index as usize))
      .copied(),
  )
}

/// Encrypt/decrypt against an explicit key, for tests and for anything
/// that must not depend on process-wide config.
#[cfg(test)]
fn encrypt_with(
  key: &[u8; 32],
  version: u32,
  plaintext: &str,
) -> anyhow::Result<String> {
  let cipher = XChaCha20Poly1305::new(key.into());
  let mut nonce = [0u8; NONCE_BYTES];
  OsRng.try_fill_bytes(&mut nonce)?;
  let nonce = XNonce::from_slice(&nonce);
  let ciphertext = cipher
    .encrypt(nonce, plaintext.as_bytes())
    .map_err(|e| anyhow!("{e}"))?;
  let mut payload = nonce.to_vec();
  payload.extend_from_slice(&ciphertext);
  Ok(format!("{PREFIX}{version}:{}", BASE64.encode(payload)))
}

#[cfg(test)]
fn decrypt_with(
  key: &[u8; 32],
  value: &str,
) -> anyhow::Result<String> {
  let Some((_, payload)) = parse(value) else {
    return Ok(value.to_string());
  };
  let payload = BASE64.decode(payload)?;
  let (nonce, ciphertext) = payload.split_at(NONCE_BYTES);
  let cipher = XChaCha20Poly1305::new(key.into());
  let plaintext = cipher
    .decrypt(XNonce::from_slice(nonce), ciphertext)
    .map_err(|_| anyhow!("decrypt failed"))?;
  Ok(String::from_utf8(plaintext)?)
}

#[cfg(test)]
mod tests {
  use super::*;

  const KEY: [u8; 32] = [7u8; 32];
  const OTHER_KEY: [u8; 32] = [9u8; 32];

  #[test]
  fn round_trips() {
    let value = encrypt_with(&KEY, 1, "hunter2").unwrap();
    assert_ne!(
      value, "hunter2",
      "the stored form must not be the secret"
    );
    assert!(is_encrypted(&value));
    assert_eq!(decrypt_with(&KEY, &value).unwrap(), "hunter2");
  }

  #[test]
  fn every_encryption_differs() {
    // A deterministic ciphertext would tell anyone with the database
    // which secrets are equal to each other - including which ones
    // were never changed from a shared default.
    let a = encrypt_with(&KEY, 1, "same").unwrap();
    let b = encrypt_with(&KEY, 1, "same").unwrap();
    assert_ne!(a, b);
    assert_eq!(decrypt_with(&KEY, &a).unwrap(), "same");
    assert_eq!(decrypt_with(&KEY, &b).unwrap(), "same");
  }

  #[test]
  fn the_wrong_key_fails_loudly() {
    let value = encrypt_with(&KEY, 1, "hunter2").unwrap();
    assert!(
      decrypt_with(&OTHER_KEY, &value).is_err(),
      "a wrong key must error, never return garbage a caller might store"
    );
  }

  #[test]
  fn tampering_is_detected() {
    // AEAD, not just encryption: someone with write access to the
    // database must not be able to flip bits in a credential.
    let value = encrypt_with(&KEY, 1, "hunter2").unwrap();
    let (head, payload) = value.rsplit_once(':').unwrap();
    let mut bytes = BASE64.decode(payload).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    let tampered = format!("{head}:{}", BASE64.encode(bytes));
    assert!(decrypt_with(&KEY, &tampered).is_err());
  }

  #[test]
  fn plaintext_passes_through() {
    // The no-migration story: a row written before encryption was
    // switched on still reads back as itself.
    assert_eq!(
      decrypt_with(&KEY, "written-before").unwrap(),
      "written-before"
    );
  }

  #[test]
  fn a_value_survives_its_key_stopping_being_newest() {
    // Rotation is append-only. A value written by v1 must still
    // decrypt after v2 becomes the key new writes use.
    let old = encrypt_with(&KEY, 1, "old-secret").unwrap();
    assert_eq!(parse(&old).unwrap().0, 1);
    let new = encrypt_with(&OTHER_KEY, 2, "new-secret").unwrap();
    assert_eq!(parse(&new).unwrap().0, 2);
    assert_eq!(decrypt_with(&KEY, &old).unwrap(), "old-secret");
    assert_eq!(decrypt_with(&OTHER_KEY, &new).unwrap(), "new-secret");
  }

  #[test]
  fn rotation_makes_the_old_key_droppable() {
    // The property ReencryptSecrets exists to deliver: after the
    // pass, a value written by v1 is readable with ONLY v2
    // configured, so v1 can be removed from secret_keys. Without the
    // pass, dropping v1 strands the value forever.
    let stored_by_v1 = encrypt_with(&KEY, 1, "rotate-me").unwrap();
    assert_eq!(parse(&stored_by_v1).unwrap().0, 1);

    // What reencrypted() does: read with whatever wrote it, write
    // with the newest.
    let plaintext = decrypt_with(&KEY, &stored_by_v1).unwrap();
    let stored_by_v2 =
      encrypt_with(&OTHER_KEY, 2, &plaintext).unwrap();

    assert_eq!(parse(&stored_by_v2).unwrap().0, 2);
    assert_eq!(
      decrypt_with(&OTHER_KEY, &stored_by_v2).unwrap(),
      "rotate-me",
      "v2 alone must be able to read it - that is the whole point"
    );
    assert!(
      decrypt_with(&KEY, &stored_by_v2).is_err(),
      "and it is genuinely no longer v1's ciphertext"
    );
  }

  #[test]
  fn key_version_distinguishes_current_from_stale_and_plaintext() {
    // reencrypted() skips on Some(newest). If key_version were wrong
    // the pass would either churn every secret on every run, or skip
    // values it was supposed to rotate.
    let v1 = encrypt_with(&KEY, 1, "x").unwrap();
    let v2 = encrypt_with(&OTHER_KEY, 2, "x").unwrap();
    assert_eq!(key_version(&v1), Some(1));
    assert_eq!(key_version(&v2), Some(2));
    assert_eq!(
      key_version("written-before-encryption"),
      None,
      "plaintext must not report a version, or the pass would skip it"
    );
  }

  #[test]
  fn plaintext_is_recognised_as_plaintext() {
    // The whole no-migration story rests on this: anything without the
    // envelope is a value written before encryption was turned on.
    assert!(!is_encrypted("hunter2"));
    assert!(!is_encrypted(""));
    assert!(!is_encrypted("komodo:enc:"));
    assert!(!is_encrypted("komodo:enc:v:payload"));
    assert!(is_encrypted("komodo:enc:v1:cGF5bG9hZA=="));
    assert!(is_encrypted("komodo:enc:v12:cGF5bG9hZA=="));
  }

  #[test]
  fn parses_version_and_payload() {
    assert_eq!(parse("komodo:enc:v3:abc"), Some((3, "abc")));
    assert_eq!(parse("nope"), None);
  }
}

#[cfg(test)]
mod encrypt_if_set_tests {
  use super::*;

  /// Absence must survive the round trip. Encrypting "" yields
  /// non-empty ciphertext that decrypts back to "", so a consumer sees a
  /// field that is present and blank rather than absent - and reports a
  /// half-configured credential for an account that never had one.
  #[test]
  fn an_empty_value_stays_empty() {
    assert_eq!(encrypt_if_set("").unwrap(), "");
    assert!(
      !is_encrypted(&encrypt_if_set("").unwrap()),
      "an empty field must not become ciphertext"
    );
  }
}
