//! Core resolves ssh / TLS material for its OWN clones.
//!
//! Everything in git_ssh.rs and git_tls.rs goes through Periphery, which
//! resolves credentials from its own config file. Core clones separately
//! - resource syncs, and Stacks/Builds/Repos reading remote config - and
//! until `apply_git_auth` existed those args always carried `None`, so a
//! provider reachable only over ssh worked for server-side operations
//! and silently failed for anything Core fetched itself.
//!
//! These use DB-stored accounts (CreateGitProviderAccount), which is the
//! path Periphery's config-file fixtures cannot exercise, and a
//! ResourceSync, whose repo Core clones itself.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::RunSync,
    write::{
      CreateGitProviderAccount, CreateResourceSync,
      DeleteGitProviderAccount, DeleteResourceSync,
    },
  },
  entities::{
    provider::_PartialGitProviderAccount,
    sync::PartialResourceSyncConfig,
  },
};
use komodo_e2e::{authenticated_client, e2e_env, finished_update};

/// Run a sync whose repo lives on `domain`, and return every log as text.
async fn sync_logs(
  client: &komodo_client::KomodoClient,
  name: &str,
  domain: &str,
  account: &str,
) -> String {
  let sync = client
    .write(CreateResourceSync {
      name: name.to_string(),
      config: PartialResourceSyncConfig {
        git_provider: Some(domain.into()),
        git_account: Some(account.into()),
        repo: Some("core-group/core-repo".into()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create resource sync");

  let update = client
    .execute(RunSync {
      sync: sync.id.clone(),
      resource_type: None,
      resources: None,
      // dry_run still clones - it runs the real code path right up to
      // the point of mutation, which is exactly the part under test,
      // without touching any resource in the e2e stack.
      dry_run: true,
    })
    .await
    .expect("RunSync should be dispatchable");
  let finished = finished_update(client, &update.id)
    .await
    .expect("the sync update should finish");

  let text = finished
    .logs
    .iter()
    .map(|log| {
      format!(
        "{}\n{}\n{}\n{}",
        log.stage, log.command, log.stdout, log.stderr
      )
    })
    .collect::<Vec<_>>()
    .join("\n---\n");

  client.write(DeleteResourceSync { id: sync.id }).await.ok();
  text
}

/// An ssh key on a DB account must reach Core's own clone, which shows
/// up as an scp-style remote. Anything else means apply_git_auth did not
/// run or did not find the account.
#[tokio::test]
async fn core_resolves_an_ssh_key_for_its_own_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("core-ssh.e2e.invalid".into()),
        username: Some("core-ssh".into()),
        token: Some(String::new()),
        ssh_private_key: Some(
          "-----BEGIN OPENSSH PRIVATE KEY-----\nCoreSideSshFixture\n-----END OPENSSH PRIVATE KEY-----".into(),
        ),
        ssh_known_hosts: Some(
          "core-ssh.e2e.invalid ssh-ed25519 AAAAC3NotReal".into(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let text = sync_logs(
    &client,
    "e2e-core-ssh",
    "core-ssh.e2e.invalid",
    "core-ssh",
  )
  .await;

  assert!(
    text.contains("git@core-ssh.e2e.invalid:core-group/core-repo"),
    "Core's own clone did not use the account's ssh key - the remote is \
     not scp-style, so apply_git_auth never populated args.ssh. \
     Logs:\n{text}"
  );
  // The key is encrypted at rest, so a failure to DECRYPT would also
  // show here rather than silently falling back to https.
  assert!(
    !text.contains("CoreSideSshFixture"),
    "the ssh key leaked into the update logs. Logs:\n{text}"
  );

  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}

/// Same path, TLS material: the -c http.ssl* flags must reach the git
/// invocation Core issues.
#[tokio::test]
async fn core_resolves_tls_material_for_its_own_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("core-tls.e2e.invalid".into()),
        username: Some("core-tls".into()),
        token: Some(String::new()),
        tls_ca_bundle: Some(
          "-----BEGIN CERTIFICATE-----\nCoreSideCaFixture\n-----END CERTIFICATE-----".into(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let text = sync_logs(
    &client,
    "e2e-core-tls",
    "core-tls.e2e.invalid",
    "core-tls",
  )
  .await;

  assert!(
    text.contains("http.sslCAInfo="),
    "Core's own clone did not receive the account's CA bundle. \
     Logs:\n{text}"
  );
  assert!(
    !text.contains("CoreSideCaFixture"),
    "the CA contents reached the command line instead of a path. \
     Logs:\n{text}"
  );

  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}

/// The control. An account with neither must leave Core's clone exactly
/// as it was - plain https, no ssl flags. Without this, both tests above
/// would pass just as happily if Komodo attached material to every
/// clone.
#[tokio::test]
async fn an_account_with_no_material_changes_nothing() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("core-plain.e2e.invalid".into()),
        username: Some("core-plain".into()),
        token: Some("glpat-not-real".into()),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let text = sync_logs(
    &client,
    "e2e-core-plain",
    "core-plain.e2e.invalid",
    "core-plain",
  )
  .await;

  assert!(
    !text.contains("git@core-plain.e2e.invalid"),
    "an account with no ssh key produced an ssh remote. Logs:\n{text}"
  );
  for flag in ["http.sslCert=", "http.sslKey=", "http.sslCAInfo="] {
    assert!(
      !text.contains(flag),
      "{flag} was attached to an account with no TLS material. \
       Logs:\n{text}"
    );
  }

  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}
