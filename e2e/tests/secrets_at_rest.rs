//! Does the database actually hold ciphertext?
//!
//! Every other test of encryption at rest asks Core, and Core will
//! happily tell you the plaintext it just decrypted - which is exactly
//! what it would say if nothing were encrypted at all. So these tests
//! write through the API and then read the raw document straight out of
//! the database, bypassing Core entirely, and look at the bytes.
//!
//! That is the only check that fails when a write site is missed. The
//! failure mode this guards is silent by construction: the field is a
//! `String`, plaintext and ciphertext are both valid, and every
//! round-trip through the API looks identical either way.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use database::mungos::mongodb::{Client, bson::doc};
use komodo_client::{
  api::{
    read::{GetGitProviderAccount, GetVariable},
    write::{
      CreateGitProviderAccount, CreateVariable,
      DeleteGitProviderAccount, DeleteVariable, ReencryptSecrets,
      UpdateVariableIsSecret, UpdateVariableValue,
    },
  },
  entities::provider::_PartialGitProviderAccount,
};
use komodo_e2e::{authenticated_client, e2e_env};

/// The envelope [komodo_core::crypto] writes. Duplicated rather than
/// imported because importing it from Core would make this test pass
/// by agreeing with the implementation instead of by observation.
const ENVELOPE: &str = "komodo:enc:v";

/// Raw read of a single field, straight from the database.
///
/// `None` only when the harness cannot reach a database at all - these
/// tests skip when run against an environment they cannot inspect.
///
/// The key deliberately does NOT get the same treatment. Skipping when
/// it is missing would make the whole file pass on an instance with
/// encryption switched off, which is the precise state it exists to
/// detect. Reachable database, no key configured, is a failure.
async fn stored_field(
  collection: &str,
  filter: bson::Document,
  field: &str,
) -> Option<String> {
  let address = std::env::var("KOMODO_E2E_DATABASE_ADDRESS").ok()?;
  assert!(
    std::env::var("KOMODO_E2E_SECRET_KEY")
      .is_ok_and(|key| !key.is_empty()),
    "KOMODO_E2E_SECRET_KEY is unset, so Core is storing secrets in \
     the clear. These tests would pass on an unencrypted instance if \
     they skipped here."
  );
  let client = Client::with_uri_str(format!("mongodb://{address}"))
    .await
    .expect("failed to connect to the e2e database");
  let doc = client
    .database("komodo")
    .collection::<bson::Document>(collection)
    .find_one(filter)
    .await
    .expect("failed to read the raw document")
    .expect("no such document - did the API write actually land?");
  Some(
    doc
      .get_str(field)
      .unwrap_or_else(|_| panic!("{field} is not a string"))
      .to_string(),
  )
}

#[tokio::test]
async fn a_secret_variable_is_ciphertext_in_the_database() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let name = "e2e_secret_at_rest";
  let plaintext = "correct-horse-battery-staple";
  let _ = client.write(DeleteVariable { name: name.into() }).await;

  client
    .write(CreateVariable {
      name: name.into(),
      value: plaintext.into(),
      description: "written by the e2e suite".into(),
      is_secret: true,
    })
    .await
    .expect("failed to create the variable");

  let Some(stored) =
    stored_field("Variable", doc! { "name": name }, "value").await
  else {
    eprintln!("SKIP secrets_at_rest: no database address / key");
    return;
  };

  assert!(
    stored.starts_with(ENVELOPE),
    "the database holds {stored:?}, which is not an encrypted value"
  );
  assert!(
    !stored.contains(plaintext),
    "the plaintext is sitting in the database"
  );

  // ... and Core still gives it back, so this bought security and not
  // just an unreadable database.
  let read = client
    .read(GetVariable { name: name.into() })
    .await
    .expect("failed to read the variable back");
  assert_eq!(read.value, plaintext);

  client
    .write(DeleteVariable { name: name.into() })
    .await
    .ok();
}

#[tokio::test]
async fn flipping_is_secret_on_encrypts_the_existing_value() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  // The transition is the interesting case: a value that already exists
  // as plaintext, marked secret afterwards. Encrypting only on writes
  // to `value` would leave it in the clear while the UI shows a mask.
  let name = "e2e_secret_transition";
  let plaintext = "was-not-secret-then-was";
  let _ = client.write(DeleteVariable { name: name.into() }).await;

  client
    .write(CreateVariable {
      name: name.into(),
      value: plaintext.into(),
      description: String::new(),
      is_secret: false,
    })
    .await
    .expect("failed to create the variable");

  let Some(stored) =
    stored_field("Variable", doc! { "name": name }, "value").await
  else {
    eprintln!("SKIP secrets_at_rest: no database address / key");
    return;
  };
  assert_eq!(
    stored, plaintext,
    "a non-secret variable should not be encrypted"
  );

  client
    .write(UpdateVariableIsSecret {
      name: name.into(),
      is_secret: true,
    })
    .await
    .expect("failed to mark the variable secret");

  let stored =
    stored_field("Variable", doc! { "name": name }, "value")
      .await
      .unwrap();
  assert!(
    stored.starts_with(ENVELOPE),
    "marking a variable secret left {stored:?} in the clear"
  );

  // And back off again: the value has to survive the round trip, or
  // un-marking a variable destroys it.
  client
    .write(UpdateVariableIsSecret {
      name: name.into(),
      is_secret: false,
    })
    .await
    .expect("failed to un-mark the variable");
  let stored =
    stored_field("Variable", doc! { "name": name }, "value")
      .await
      .unwrap();
  assert_eq!(
    stored, plaintext,
    "the value did not survive being un-marked as secret"
  );

  client
    .write(DeleteVariable { name: name.into() })
    .await
    .ok();
}

#[tokio::test]
async fn updating_a_secret_variable_rewrites_it_encrypted() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let name = "e2e_secret_update";
  let updated = "second-value";
  let _ = client.write(DeleteVariable { name: name.into() }).await;

  client
    .write(CreateVariable {
      name: name.into(),
      value: "first-value".into(),
      description: String::new(),
      is_secret: true,
    })
    .await
    .expect("failed to create the variable");
  client
    .write(UpdateVariableValue {
      name: name.into(),
      value: updated.into(),
    })
    .await
    .expect("failed to update the variable");

  let Some(stored) =
    stored_field("Variable", doc! { "name": name }, "value").await
  else {
    eprintln!("SKIP secrets_at_rest: no database address / key");
    return;
  };
  assert!(
    stored.starts_with(ENVELOPE),
    "the update wrote {stored:?} in the clear"
  );
  assert!(!stored.contains(updated));

  let read = client
    .read(GetVariable { name: name.into() })
    .await
    .expect("failed to read the variable back");
  assert_eq!(read.value, updated);

  client
    .write(DeleteVariable { name: name.into() })
    .await
    .ok();
}

#[tokio::test]
async fn a_git_provider_token_is_ciphertext_in_the_database() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let username = "e2e-at-rest";
  let token = "glpat-not-a-real-token";

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("e2e.invalid".into()),
        username: Some(username.into()),
        token: Some(token.into()),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let Some(stored) = stored_field(
    "GitProviderAccount",
    doc! { "username": username },
    "token",
  )
  .await
  else {
    eprintln!("SKIP secrets_at_rest: no database address / key");
    client
      .write(DeleteGitProviderAccount {
        id: account.id.clone(),
      })
      .await
      .ok();
    return;
  };

  assert!(
    stored.starts_with(ENVELOPE),
    "the database holds {stored:?}, which is not an encrypted token"
  );
  assert!(!stored.contains(token), "the token is in the database");

  // The read path redacts rather than decrypting, so what an admin sees
  // must be the redaction - never the ciphertext, which would look like
  // a working token to anyone copying it.
  let read = client
    .read(GetGitProviderAccount {
      id: account.id.clone(),
    })
    .await
    .expect("failed to read the account back");
  assert!(
    !read.token.starts_with(ENVELOPE),
    "ciphertext leaked to the API as {:?}",
    read.token
  );
  assert_ne!(read.token, token, "the token was returned unredacted");

  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}

#[tokio::test]
async fn reencrypt_secrets_rewrites_and_is_idempotent() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let name = "e2e_reencrypt";
  let plaintext = "value-to-rotate";
  let _ = client.write(DeleteVariable { name: name.into() }).await;

  client
    .write(CreateVariable {
      name: name.into(),
      value: plaintext.into(),
      description: String::new(),
      is_secret: true,
    })
    .await
    .expect("failed to create the variable");

  let Some(before) =
    stored_field("Variable", doc! { "name": name }, "value").await
  else {
    eprintln!("SKIP secrets_at_rest: no database address / key");
    return;
  };

  // Everything is already written by the newest key, so the pass has
  // nothing to rotate. That is the case that must be a no-op rather
  // than a full rewrite - otherwise every run churns every secret and
  // generates fresh nonces for no reason.
  let res = client
    .write(ReencryptSecrets { dry_run: false })
    .await
    .expect("ReencryptSecrets should succeed");
  assert_eq!(
    res.variables, 0,
    "nothing needed rotating, got {res:?}"
  );
  assert!(
    res.already_current > 0,
    "the secret should have been counted as current, got {res:?}"
  );
  assert!(res.failed.is_empty(), "unexpected failures: {res:?}");

  let after =
    stored_field("Variable", doc! { "name": name }, "value")
      .await
      .unwrap();
  assert_eq!(
    before, after,
    "an already-current value must not be rewritten"
  );

  // Still encrypted, still readable.
  assert!(after.starts_with(ENVELOPE));
  let read = client
    .read(GetVariable { name: name.into() })
    .await
    .expect("failed to read the variable back");
  assert_eq!(read.value, plaintext);

  client
    .write(DeleteVariable { name: name.into() })
    .await
    .ok();
}

#[tokio::test]
async fn reencrypt_secrets_dry_run_writes_nothing() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let res = client
    .write(ReencryptSecrets { dry_run: true })
    .await
    .expect("dry run should succeed");
  assert!(res.dry_run, "the response should say it was a dry run");
  assert!(res.failed.is_empty(), "unexpected failures: {res:?}");
}

/// The ssh key and the TLS client key are credentials exactly as much as
/// the token is, and they arrived later - which is precisely how a field
/// ends up sitting in plaintext beside an encrypted one while
/// encryption-at-rest still looks enabled.
///
/// Asserts on every new field at once, including the ones that must NOT
/// be encrypted: the certificate, the CA bundle and known_hosts are
/// public by nature, and encrypting them would leave an operator unable
/// to read back what they configured while buying no secrecy.
#[tokio::test]
async fn the_ssh_and_tls_keys_are_ciphertext_in_the_database() {
  let Some(env) = e2e_env() else {
    eprintln!("SKIP secrets_at_rest: KOMODO_ADDRESS not set");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let username = "e2e-at-rest-keys";
  let ssh_key = "-----BEGIN OPENSSH PRIVATE KEY-----\nAtRestSshSecret\n-----END OPENSSH PRIVATE KEY-----";
  let tls_key = "-----BEGIN PRIVATE KEY-----\nAtRestTlsSecret\n-----END PRIVATE KEY-----";
  let ca = "-----BEGIN CERTIFICATE-----\nAtRestPublicCa\n-----END CERTIFICATE-----";
  let known_hosts =
    "e2e.invalid ssh-ed25519 AAAAC3AtRestPublicHostKey";

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("e2e-keys.invalid".into()),
        username: Some(username.into()),
        token: Some("glpat-also-not-real".into()),
        ssh_private_key: Some(ssh_key.into()),
        ssh_known_hosts: Some(known_hosts.into()),
        tls_client_key: Some(tls_key.into()),
        tls_ca_bundle: Some(ca.into()),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let cleanup = |id: String| async move {
    let client =
      authenticated_client(&e2e_env().unwrap()).await.unwrap();
    client.write(DeleteGitProviderAccount { id }).await.ok();
  };

  for (field, plaintext) in
    [("ssh_private_key", ssh_key), ("tls_client_key", tls_key)]
  {
    let Some(stored) = stored_field(
      "GitProviderAccount",
      doc! { "username": username },
      field,
    )
    .await
    else {
      eprintln!("SKIP secrets_at_rest: no database address / key");
      cleanup(account.id.clone()).await;
      return;
    };
    assert!(
      stored.starts_with(ENVELOPE),
      "{field} holds {stored:?}, which is not encrypted"
    );
    assert!(
      !stored.contains(plaintext),
      "{field} is in the database in plaintext"
    );
  }

  // The other side of the same coin. These are not secrets, and
  // encrypting them would cost readability for nothing.
  for (field, expected) in
    [("ssh_known_hosts", known_hosts), ("tls_ca_bundle", ca)]
  {
    let Some(stored) = stored_field(
      "GitProviderAccount",
      doc! { "username": username },
      field,
    )
    .await
    else {
      cleanup(account.id.clone()).await;
      return;
    };
    assert_eq!(
      stored, expected,
      "{field} is not a secret and must be stored as written, so an \
       operator can read back what they configured"
    );
  }

  cleanup(account.id).await;
}
