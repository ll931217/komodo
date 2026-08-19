//! SSH git remotes, end to end.
//!
//! No ssh server is involved and none is needed. Every assertion here is
//! about which path Komodo takes and what it does with the key - the
//! remote URL form, the host-key policy, whether key material leaks into
//! a log. A real handshake would test OpenSSH, not Komodo, and would
//! require shipping a usable private key in the repo to do it.
//!
//! The fixtures live in e2e/periphery.config.toml. If that file ever
//! stops parsing, `args.ssh` is never populated, the remote falls back to
//! https, and `the_remote_url_switches_to_scp_form` fails - so a broken
//! fixture surfaces as a failure rather than as a quietly weaker test.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::CloneRepo,
    read::ListServers,
    write::{CreateRepo, DeleteRepo},
  },
  entities::repo::PartialRepoConfig,
};
use komodo_e2e::{authenticated_client, e2e_env, finished_update};

/// Text of every log on the update, so assertions can look at the
/// command git was given as well as its output.
async fn clone_logs(
  client: &komodo_client::KomodoClient,
  name: &str,
  domain: &str,
  account: &str,
) -> (String, String) {
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  let repo = client
    .write(CreateRepo {
      name: name.to_string(),
      config: PartialRepoConfig {
        server_id: Some(server_id),
        git_provider: Some(domain.into()),
        git_account: Some(account.into()),
        repo: Some("e2e-group/e2e-repo".into()),
        ..Default::default()
      },
    })
    .await
    .expect("Failed to create repo");

  let update = client
    .execute(CloneRepo {
      repo: repo.id.clone(),
    })
    .await
    .expect("CloneRepo should be dispatchable");
  let finished = finished_update(client, &update.id)
    .await
    .expect("the clone update should finish");

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

  client.write(DeleteRepo { id: repo.id }).await.ok();
  (text, format!("{}", finished.success))
}

/// The load-bearing one. An ssh account must produce an scp-style remote,
/// because that is what proves the whole chain ran: the account was
/// found, `args.ssh` was populated, and `remote_url` took the ssh branch.
/// If any link were missing the remote would silently be https and the
/// clone would fail for an entirely different reason.
#[tokio::test]
async fn the_remote_url_switches_to_scp_form() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let (text, _) = clone_logs(
    &client,
    "e2e-ssh-scp-form",
    "ssh-tofu.e2e.invalid",
    "e2e-ssh-tofu",
  )
  .await;

  assert!(
    text.contains("git@ssh-tofu.e2e.invalid:e2e-group/e2e-repo"),
    "expected an scp-style ssh remote. Its absence means the ssh \
     material never reached remote_url - check that \
     e2e/periphery.config.toml still parses. Logs:\n{text}"
  );
  assert!(
    !text.contains("https://ssh-tofu.e2e.invalid"),
    "the remote fell back to https despite an ssh key being \
     configured. Logs:\n{text}"
  );
}

/// Strict is the default, and a provider with no known_hosts under it
/// must fail with a message about known_hosts - not hang on a prompt,
/// and not quietly downgrade to accepting any host key.
#[tokio::test]
async fn strict_checking_without_known_hosts_refuses_rather_than_downgrades()
 {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let (text, success) = clone_logs(
    &client,
    "e2e-ssh-strict",
    "ssh-strict.e2e.invalid",
    "e2e-ssh-strict",
  )
  .await;

  assert_eq!(
    success, "false",
    "a clone that cannot verify the host key must not succeed. \
     Logs:\n{text}"
  );
  assert!(
    text.contains("known_hosts"),
    "the failure must name the missing known_hosts so an operator can \
     act on it, rather than surfacing as an opaque ssh error. \
     Logs:\n{text}"
  );
  assert!(
    !text.contains("StrictHostKeyChecking=no"),
    "host key verification was disabled somewhere in the fallback path. \
     Logs:\n{text}"
  );
}

/// Whichever path runs, the key itself must never reach a log. Update
/// logs are readable by anyone who can see the resource in the UI.
#[tokio::test]
async fn the_private_key_never_reaches_the_update_logs() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let (text, _) = clone_logs(
    &client,
    "e2e-ssh-no-leak",
    "ssh-tofu.e2e.invalid",
    "e2e-ssh-tofu",
  )
  .await;

  assert!(
    !text.contains("ThisIsNotARealKey"),
    "the ssh private key leaked into the update logs. Logs:\n{text}"
  );
  assert!(
    !text.contains("BEGIN OPENSSH PRIVATE KEY"),
    "key material leaked into the update logs. Logs:\n{text}"
  );
}

/// The key file is written to disk for the duration of the command, so
/// the thing worth asserting is that nothing is left behind afterwards.
#[tokio::test]
async fn no_key_material_is_left_on_disk_after_the_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let (_text, _) = clone_logs(
    &client,
    "e2e-ssh-cleanup",
    "ssh-tofu.e2e.invalid",
    "e2e-ssh-tofu",
  )
  .await;

  // Periphery runs as a host process in the e2e stack and uses the same
  // temp dir, so the leftovers would be visible here.
  let leftovers = std::fs::read_dir(std::env::temp_dir())
    .expect("temp dir readable")
    .filter_map(Result::ok)
    .map(|entry| entry.file_name().to_string_lossy().to_string())
    .filter(|name| name.starts_with("komodo-ssh-e2e-ssh-cleanup"))
    .collect::<Vec<_>>();

  assert!(
    leftovers.is_empty(),
    "ssh session directories survived the clone: {leftovers:?}"
  );
}
