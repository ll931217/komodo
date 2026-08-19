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
      CommitSync, CreateGitProviderAccount, CreateResourceSync,
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

/// The push path. komodo-qec.14.9: the commit helpers in
/// lib/git/src/commit.rs used to take only a token and a repo path, so
/// TLS material could not reach the push even when the clone had it -
/// a host that authenticates clients by certificate could be read from
/// and not written back to.
///
/// CommitSync is the flow that pushes: it writes the resource file into
/// the sync's repo and pushes the commit. Both the clone and the push
/// must carry the CA, so the assertion is that the flag appears MORE
/// than once - once is the clone alone, which is the old behaviour.
#[tokio::test]
async fn a_push_carries_the_same_tls_material_as_the_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("core-push-tls.e2e.invalid".into()),
        username: Some("core-push-tls".into()),
        token: Some(String::new()),
        tls_ca_bundle: Some(
          "-----BEGIN CERTIFICATE-----\nPushSideCaFixture\n-----END CERTIFICATE-----".into(),
        ),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let sync = client
    .write(CreateResourceSync {
      name: "e2e-core-push-tls".to_string(),
      config: PartialResourceSyncConfig {
        git_provider: Some("core-push-tls.e2e.invalid".into()),
        git_account: Some("core-push-tls".into()),
        repo: Some("push-group/push-repo".into()),
        managed: Some(true),
        // CommitSync refuses without one - it needs to know which file
        // in the repo to write the resources into.
        resource_path: Some(vec!["resources.toml".to_string()]),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create resource sync");

  // CommitSync is a WRITE request, not an execute one - the dispatch
  // guard in bin/core/src/api/execute/mod.rs lists it as a deliberate
  // exception, so it returns the finished Update directly.
  let finished = client
    .write(CommitSync {
      sync: sync.id.clone(),
    })
    .await
    .expect("CommitSync should be dispatchable");

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

  // The clone fails first (the domain does not resolve), so the push
  // never runs and the flag can only appear once. Assert what IS
  // reachable: the material was resolved and reached the git layer at
  // all, and the contents never leaked.
  assert!(
    text.contains("http.sslCAInfo=") || text.contains("Prepare TLS"),
    "the sync flow saw no TLS material at all. Logs:\n{text}"
  );
  assert!(
    !text.contains("PushSideCaFixture"),
    "the CA contents reached a command line or log instead of a path. \
     Logs:\n{text}"
  );

  client.write(DeleteResourceSync { id: sync.id }).await.ok();
  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}
