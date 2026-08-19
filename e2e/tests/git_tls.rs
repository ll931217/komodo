//! TLS client certificates and custom CA bundles, end to end.
//!
//! No TLS server is involved. The assertions are about which `-c
//! http.ssl*` flags git is given, that only PATHS reach the command
//! line, and - the one the bead specifically asks for - that material
//! configured for one provider does not reach a clone of another.

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

async fn clone_logs(
  client: &komodo_client::KomodoClient,
  name: &str,
  domain: &str,
  account: &str,
) -> String {
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
  text
}

/// Proves the whole chain: account found, args.tls populated, session
/// written, flags rendered onto the git invocation.
#[tokio::test]
async fn the_clone_carries_the_tls_config_flags() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let text = clone_logs(
    &client,
    "e2e-tls-flags",
    "tls-certs.e2e.invalid",
    "e2e-tls",
  )
  .await;

  for flag in ["http.sslCert=", "http.sslKey=", "http.sslCAInfo="] {
    assert!(
      text.contains(flag),
      "expected {flag} on the git invocation. Its absence means the TLS \
       material never reached the command - check that \
       e2e/periphery.config.toml still parses. Logs:\n{text}"
    );
  }
}

/// The bead's requirement in as many words: a different provider's clone
/// must NOT receive this CA. Per-invocation `-c` flags make that
/// structural, but structural-by-construction is exactly the kind of
/// claim that quietly stops being true.
#[tokio::test]
async fn material_does_not_leak_into_another_providers_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let text = clone_logs(
    &client,
    "e2e-tls-isolation",
    "tls-none.e2e.invalid",
    "e2e-tls-none",
  )
  .await;

  for flag in ["http.sslCert=", "http.sslKey=", "http.sslCAInfo="] {
    assert!(
      !text.contains(flag),
      "{flag} reached a provider that has no TLS material configured, \
       so trusting an internal CA for one host weakened another. \
       Logs:\n{text}"
    );
  }
  assert!(
    !text.contains("sslVerify=false"),
    "verification was disabled as a fallback. Logs:\n{text}"
  );
}

/// Only paths may reach the command line; PEM contents are as sensitive
/// as a token and update logs are readable in the UI.
#[tokio::test]
async fn the_pem_material_never_reaches_the_update_logs() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let text = clone_logs(
    &client,
    "e2e-tls-no-leak",
    "tls-certs.e2e.invalid",
    "e2e-tls",
  )
  .await;

  for secret in [
    "E2eFixtureClientKey",
    "E2eFixtureClientCertificate",
    "E2eFixtureInternalCa",
    "BEGIN PRIVATE KEY",
  ] {
    assert!(
      !text.contains(secret),
      "TLS material leaked into the update logs: {secret}. \
       Logs:\n{text}"
    );
  }
}

/// Written to disk for the command's duration, so what matters is that
/// nothing survives it.
#[tokio::test]
async fn no_tls_material_is_left_on_disk_after_the_clone() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let _ = clone_logs(
    &client,
    "e2e-tls-cleanup",
    "tls-certs.e2e.invalid",
    "e2e-tls",
  )
  .await;

  let leftovers = std::fs::read_dir(std::env::temp_dir())
    .expect("temp dir readable")
    .filter_map(Result::ok)
    .map(|entry| entry.file_name().to_string_lossy().to_string())
    .filter(|name| name.starts_with("komodo-tls-e2e-tls-cleanup"))
    .collect::<Vec<_>>();

  assert!(
    leftovers.is_empty(),
    "TLS session directories survived the clone: {leftovers:?}"
  );
}
