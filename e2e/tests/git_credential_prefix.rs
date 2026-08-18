//! Credential selection by repo-path prefix, exercised through the real
//! resolution path rather than the matcher in isolation.
//!
//! The selection ALGORITHM is unit-tested in
//! client/core/rs/src/entities/credential_match.rs - 11 cases including
//! the segment-boundary rule. What those tests cannot show is that Core
//! actually reaches the prefix branch when a resource names no git
//! account, because that depends on 18 call sites threading the repo
//! path and on the DB lookup fetching candidates rather than doing an
//! exact-match find_one. A clean compile proves neither.
//!
//! So this asserts on the AMBIGUITY error. It is the one outcome that
//! only the prefix branch can produce: two accounts sharing a prefix is
//! meaningless to the exact-match path, which never compares prefixes at
//! all. Seeing that error means the resource's repo path reached
//! git_token_by_prefix, was matched against DB candidates, and the
//! conflict was reported instead of guessed.
//!
//! Proving the positive case end to end would need a real private remote
//! whose credential Komodo could be observed using, which the e2e stack
//! has no way to provide - asserting on a clone failure message would be
//! testing git's error text, not Komodo's selection.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_client::{
  api::{
    execute::CloneRepo,
    read::ListServers,
    write::{
      CreateGitProviderAccount, CreateRepo, DeleteGitProviderAccount,
      DeleteRepo,
    },
  },
  entities::{
    provider::_PartialGitProviderAccount, repo::PartialRepoConfig,
  },
};
use komodo_e2e::{authenticated_client, e2e_env, finished_update};

const DOMAIN: &str = "prefix-e2e.invalid";

#[tokio::test]
async fn two_accounts_sharing_a_prefix_report_the_conflict() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  // Two accounts, same domain, SAME prefix. Distinct usernames, because
  // the unique index is on (domain, username) - the conflict this
  // creates is at the prefix level, which nothing else guards.
  let mut accounts = Vec::new();
  for username in ["prefix-team-a", "prefix-team-b"] {
    accounts.push(
      client
        .write(CreateGitProviderAccount {
          account: _PartialGitProviderAccount {
            domain: Some(DOMAIN.into()),
            username: Some(username.into()),
            token: Some("not-a-real-token".into()),
            path_prefix: Some("shared-group".into()),
            ..Default::default()
          },
        })
        .await
        .expect("failed to create the git provider account"),
    );
  }

  // A Repo naming NO git account, whose path falls under that prefix.
  // git_account is left unset on purpose - that is the branch which used
  // to return "no credential" unconditionally.
  let repo = client
    .write(CreateRepo {
      name: "e2e-prefix-conflict".to_string(),
      config: PartialRepoConfig {
        server_id: Some(server_id),
        git_provider: Some(DOMAIN.into()),
        repo: Some("shared-group/some-repo".into()),
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
  let finished = finished_update(&client, &update.id)
    .await
    .expect("the clone update should finish");

  assert!(
    !finished.success,
    "an unresolvable credential must not be reported as a successful \
     clone: {finished:?}"
  );

  let text = finished
    .logs
    .iter()
    .map(|log| {
      format!("{}\n{}\n{}", log.stage, log.stdout, log.stderr)
    })
    .collect::<Vec<_>>()
    .join("\n");

  assert!(
    text.contains("same path prefix"),
    "expected the prefix-ambiguity error, which only the prefix branch \
     can produce. Without it, the repo path never reached \
     git_token_by_prefix. Logs:\n{text}"
  );
  assert!(
    text.contains("prefix-team-a") && text.contains("prefix-team-b"),
    "the error must name both competing accounts so an operator knows \
     which config to fix. Logs:\n{text}"
  );

  client.write(DeleteRepo { id: repo.id }).await.ok();
  for account in accounts {
    client
      .write(DeleteGitProviderAccount { id: account.id })
      .await
      .ok();
  }
}

/// The control. Same repo path, ONE account holding the prefix, so
/// selection is unambiguous - the ambiguity error must not appear.
///
/// Without this, the test above would pass just as happily if Komodo
/// reported "same path prefix" on every prefix lookup regardless of
/// whether there was a conflict.
#[tokio::test]
async fn one_account_holding_the_prefix_is_not_a_conflict() {
  let Some(env) = e2e_env() else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  let client = authenticated_client(&env).await.unwrap();
  let server_id = client
    .read(ListServers::default())
    .await
    .expect("Failed to list servers")
    .pop()
    .expect("Expected the init-registered first server")
    .id;

  let account = client
    .write(CreateGitProviderAccount {
      account: _PartialGitProviderAccount {
        domain: Some("prefix-solo-e2e.invalid".into()),
        username: Some("prefix-solo".into()),
        token: Some("not-a-real-token".into()),
        path_prefix: Some("solo-group".into()),
        ..Default::default()
      },
    })
    .await
    .expect("failed to create the git provider account");

  let repo = client
    .write(CreateRepo {
      name: "e2e-prefix-solo".to_string(),
      config: PartialRepoConfig {
        server_id: Some(server_id),
        git_provider: Some("prefix-solo-e2e.invalid".into()),
        repo: Some("solo-group/some-repo".into()),
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
  let finished = finished_update(&client, &update.id)
    .await
    .expect("the clone update should finish");

  let text = finished
    .logs
    .iter()
    .map(|log| {
      format!("{}\n{}\n{}", log.stage, log.stdout, log.stderr)
    })
    .collect::<Vec<_>>()
    .join("\n");

  // The clone itself still fails - the domain does not resolve and the
  // token is fake. That is fine and is not what this asserts.
  assert!(
    !text.contains("same path prefix"),
    "a single account holding the prefix is not a conflict, but Komodo \
     reported one. Logs:\n{text}"
  );
  // The token must never be echoed, whichever path resolved it.
  assert!(
    !text.contains("not-a-real-token"),
    "the token leaked into the update logs. Logs:\n{text}"
  );

  client.write(DeleteRepo { id: repo.id }).await.ok();
  client
    .write(DeleteGitProviderAccount { id: account.id })
    .await
    .ok();
}
