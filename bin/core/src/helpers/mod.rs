use std::fmt::Write;

use anyhow::{Context, anyhow};
use database::mongo_indexed::Document;
use database::mungos::{
  find::find_collect,
  mongodb::bson::{Bson, doc},
};
use indexmap::IndexSet;
use komodo_client::entities::SwarmOrServer;
use komodo_client::entities::{
  RepoExecutionArgs, ResourceTarget, SshAuth, TlsAuth,
  build::Build,
  credential_match::{PrefixCandidate, select_by_prefix},
  permission::{
    Permission, PermissionLevel, SpecificPermission, UserTarget,
  },
  repo::Repo,
  server::Server,
  stack::Stack,
  user::User,
};
use mogh_resolver::HasResponse;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::helpers::swarm::swarm_request;
use crate::{
  config::core_config, connection::PeripheryConnectionArgs,
  periphery::PeripheryClient, state::db_client,
};

pub mod action_state;
pub mod all_resources;
pub mod application;
pub mod builder;
pub mod channel;
pub mod cluster;
pub mod github_app;
pub mod image_digest;
pub mod maintenance;
pub mod ownership;
pub mod procedure;
pub mod prune;
pub mod query;
pub mod retry;
pub mod swarm;
pub mod terminal;
pub mod terraform;
pub mod update;
pub mod validations;
pub mod window;

pub fn empty_or_only_spaces(word: &str) -> bool {
  if word.is_empty() {
    return true;
  }
  for char in word.chars() {
    if char != ' ' {
      return false;
    }
  }
  true
}

/// First checks db for token, then checks core config.
/// Only errors if db call errors.
/// Returns (token, use_https)
pub async fn git_token(
  provider_domain: &str,
  account_username: &str,
  repo_path: Option<&str>,
  mut on_https_found: impl FnMut(bool),
) -> anyhow::Result<Option<String>> {
  if provider_domain.is_empty() {
    return Ok(None);
  }
  if account_username.is_empty() {
    // Nothing named on the resource. Before falling back to an
    // anonymous clone - which is what happened unconditionally until
    // now - see whether a configured account covers this repo's path.
    return git_token_by_prefix(
      provider_domain,
      repo_path,
      on_https_found,
    )
    .await;
  }
  let db_provider = db_client()
    .git_accounts
    .find_one(doc! { "domain": provider_domain, "username": account_username })
    .await
    .context("failed to query db for git provider accounts")?;
  if let Some(provider) = db_provider {
    on_https_found(provider.https);
    // A GitHub App account mints instead of storing. The `token` field
    // is ignored rather than used as a fallback: falling back would
    // silently authenticate as whatever stale PAT happened to be there
    // when the App configuration is what someone deliberately set.
    if !provider.github_app_id.is_empty() {
      return github_app_token(provider_domain, &provider)
        .await
        .map(Some);
    }
    // The one place a DB-stored git token is handed to a caller.
    return Ok(Some(
      crate::crypto::decrypt(&provider.token).with_context(|| {
        format!(
          "Failed to decrypt the git token for {account_username}@{provider_domain}"
        )
      })?,
    ));
  }
  Ok(
    core_config()
      .git_providers
      .iter()
      .find(|provider| provider.domain == provider_domain)
      .and_then(|provider| {
        on_https_found(provider.https);
        provider
          .accounts
          .iter()
          .find(|account| account.username == account_username)
          .map(|account| account.token.clone())
      }),
  )
}

/// Fallback for a resource that names no git account: pick the account
/// whose `path_prefix` is the longest segment-wise match for the repo
/// path. DB accounts are considered first, then core config, matching the
/// precedence of the named-account path above.
///
/// Deliberately NOT a `find_one`: a prefix match cannot be expressed as
/// an exact-match query, so the domain's accounts are fetched and chosen
/// among in one place. Letting Mongo return the first document by natural
/// order would make the answer depend on insertion order.
///
/// `Ok(None)` when nothing matches, because no match is the normal case
/// and means "clone anonymously", exactly as before this existed. An
/// AMBIGUOUS match is a configuration mistake and is surfaced as an
/// error, since guessing would let a config change silently redirect
/// which credential reaches a remote.
async fn git_token_by_prefix(
  provider_domain: &str,
  repo_path: Option<&str>,
  mut on_https_found: impl FnMut(bool),
) -> anyhow::Result<Option<String>> {
  let Some(repo_path) = repo_path.filter(|path| !path.is_empty())
  else {
    return Ok(None);
  };

  let db_accounts = find_collect(
    &db_client().git_accounts,
    doc! { "domain": provider_domain },
    None,
  )
  .await
  .context("failed to query db for git provider accounts")?;

  let candidates = db_accounts
    .iter()
    .map(|account| PrefixCandidate {
      username: &account.username,
      path_prefix: &account.path_prefix,
    })
    .collect::<Vec<_>>();
  if let Some(username) = select_by_prefix(&candidates, repo_path)
    .with_context(|| {
      format!(
        "Failed to select a git account for {provider_domain}/{repo_path}"
      )
    })?
    && let Some(account) =
      db_accounts.iter().find(|a| a.username == username)
  {
    on_https_found(account.https);
    return Ok(Some(
      crate::crypto::decrypt(&account.token).with_context(|| {
        format!(
          "Failed to decrypt the git token for {username}@{provider_domain}"
        )
      })?,
    ));
  }

  let Some(provider) = core_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == provider_domain)
  else {
    return Ok(None);
  };
  let candidates = provider
    .accounts
    .iter()
    .map(|account| PrefixCandidate {
      username: &account.username,
      path_prefix: &account.path_prefix,
    })
    .collect::<Vec<_>>();
  let Some(username) = select_by_prefix(&candidates, repo_path)
    .with_context(|| {
      format!(
        "Failed to select a git account for {provider_domain}/{repo_path}"
      )
    })?
  else {
    return Ok(None);
  };
  on_https_found(provider.https);
  Ok(
    provider
      .accounts
      .iter()
      .find(|account| account.username == username)
      .map(|account| account.token.clone()),
  )
}

/// Fill in `args.ssh` and `args.tls` from the git provider account
/// backing this remote.
///
/// Core resolves credentials for its OWN clones - resource syncs, and
/// Stacks/Builds/Repos/Terraform/Applications reading remote config.
/// Until this existed those args always carried `None`, so a provider
/// reachable only over ssh, or only behind an internal CA, worked for
/// Periphery-side operations and failed for anything Core cloned itself.
///
/// Resolution deliberately mirrors `git_token`: the account named on the
/// resource, else the longest matching path prefix; DB accounts before
/// core config. Token, ssh key and TLS material therefore always come
/// from the SAME account - a request half-built from two identities is a
/// miserable failure to debug.
///
/// Only fills what is empty, so material a caller already resolved wins.
/// Mint an installation token for a GitHub App account.
///
/// The App private key is encrypted at rest like every other credential,
/// so it is decrypted here and never leaves this function - what travels
/// onward is the short-lived installation token.
async fn github_app_token(
  domain: &str,
  account: &komodo_client::entities::provider::GitProviderAccount,
) -> anyhow::Result<String> {
  if account.github_app_installation_id.is_empty() {
    anyhow::bail!(
      "Git account {}@{domain} has a GitHub App id but no installation \
       id; Komodo cannot tell which installation to mint a token for",
      account.username
    );
  }
  let private_key = crate::crypto::decrypt(
    &account.github_app_private_key,
  )
  .with_context(|| {
    format!(
      "Failed to decrypt the GitHub App private key for {}@{domain}",
      account.username
    )
  })?;
  if private_key.trim().is_empty() {
    anyhow::bail!(
      "Git account {}@{domain} has a GitHub App id but no private key",
      account.username
    );
  }
  github_app::installation_token(
    domain,
    &account.github_app_id,
    &account.github_app_installation_id,
    &private_key,
  )
  .await
}

pub async fn apply_git_auth(
  args: &mut RepoExecutionArgs,
) -> anyhow::Result<()> {
  if args.ssh.is_some() && args.tls.is_some() {
    return Ok(());
  }
  if args.provider.is_empty() {
    return Ok(());
  }

  // DB first, matching the precedence git_token already has.
  let db_accounts = find_collect(
    &db_client().git_accounts,
    doc! { "domain": &args.provider },
    None,
  )
  .await
  .context("failed to query db for git provider accounts")?;

  let chosen = match &args.account {
    Some(username) if !username.is_empty() => db_accounts
      .iter()
      .find(|account| &account.username == username),
    _ => {
      let Some(repo_path) =
        args.repo.as_deref().filter(|path| !path.is_empty())
      else {
        return Ok(());
      };
      let candidates = db_accounts
        .iter()
        .map(|account| PrefixCandidate {
          username: &account.username,
          path_prefix: &account.path_prefix,
        })
        .collect::<Vec<_>>();
      match select_by_prefix(&candidates, repo_path).with_context(
        || {
          format!(
            "Failed to select a git account for {}/{repo_path}",
            args.provider
          )
        },
      )? {
        Some(username) => db_accounts
          .iter()
          .find(|account| account.username == username),
        None => None,
      }
    }
  };

  if let Some(account) = chosen {
    // Both key fields are encrypted at rest, so they need the same
    // decrypt the token gets - a raw read would hand git ciphertext.
    if args.ssh.is_none() {
      let key = decrypted_if_present(&account.ssh_private_key)
        .with_context(|| {
          format!(
            "Failed to decrypt the ssh key for {}@{}",
            account.username, args.provider
          )
        })?;
      if let Some(private_key) = key {
        args.ssh = Some(SshAuth {
          private_key,
          known_hosts: account.ssh_known_hosts.clone(),
          accept_new_host_keys: account.ssh_accept_new_host_keys,
        });
      }
    }
    if args.tls.is_none() {
      let client_key = decrypted_if_present(&account.tls_client_key)
        .with_context(|| {
          format!(
            "Failed to decrypt the TLS key for {}@{}",
            account.username, args.provider
          )
        })?
        .unwrap_or_default();
      if !account.tls_client_cert.trim().is_empty()
        || !client_key.is_empty()
        || !account.tls_ca_bundle.trim().is_empty()
      {
        args.tls = Some(TlsAuth {
          client_cert: account.tls_client_cert.clone(),
          client_key,
          ca_bundle: account.tls_ca_bundle.clone(),
        });
      }
    }
    return Ok(());
  }

  // Core config accounts are plaintext - they live in a file the
  // operator already controls, so there is nothing to decrypt.
  let Some(provider) = core_config()
    .git_providers
    .iter()
    .find(|provider| provider.domain == args.provider)
  else {
    return Ok(());
  };
  let username = match &args.account {
    Some(username) if !username.is_empty() => Some(username.clone()),
    _ => {
      let Some(repo_path) =
        args.repo.as_deref().filter(|path| !path.is_empty())
      else {
        return Ok(());
      };
      let candidates = provider
        .accounts
        .iter()
        .map(|account| PrefixCandidate {
          username: &account.username,
          path_prefix: &account.path_prefix,
        })
        .collect::<Vec<_>>();
      select_by_prefix(&candidates, repo_path)
        .with_context(|| {
          format!(
            "Failed to select a git account for {}/{repo_path}",
            args.provider
          )
        })?
        .map(str::to_string)
    }
  };
  let Some(username) = username else {
    return Ok(());
  };
  let Some(account) = provider
    .accounts
    .iter()
    .find(|account| account.username == username)
  else {
    return Ok(());
  };
  if args.ssh.is_none() && !account.ssh_private_key.trim().is_empty()
  {
    args.ssh = Some(SshAuth {
      private_key: account.ssh_private_key.clone(),
      known_hosts: account.ssh_known_hosts.clone(),
      accept_new_host_keys: account.ssh_accept_new_host_keys,
    });
  }
  if args.tls.is_none()
    && (!account.tls_client_cert.trim().is_empty()
      || !account.tls_client_key.trim().is_empty()
      || !account.tls_ca_bundle.trim().is_empty())
  {
    args.tls = Some(TlsAuth {
      client_cert: account.tls_client_cert.clone(),
      client_key: account.tls_client_key.clone(),
      ca_bundle: account.tls_ca_bundle.clone(),
    });
  }
  Ok(())
}

/// Decrypt a stored field, or `None` when it was never set.
///
/// An empty field is absence, not a zero-length secret - decrypting it
/// would fail and turn "this account has no ssh key", the common case,
/// into an error.
fn decrypted_if_present(
  value: &str,
) -> anyhow::Result<Option<String>> {
  if value.trim().is_empty() {
    return Ok(None);
  }
  // A value that decrypts to nothing is still absence. Belt and braces
  // against rows written before encrypt_if_set existed, which stored
  // ciphertext of an empty string.
  let decrypted = crate::crypto::decrypt(value)?;
  Ok((!decrypted.trim().is_empty()).then_some(decrypted))
}

pub async fn stack_git_token(
  stack: &mut Stack,
  repo: Option<&mut Repo>,
) -> anyhow::Result<Option<String>> {
  if let Some(repo) = repo {
    return git_token(
      &repo.config.git_provider,
      &repo.config.git_account,
      Some(&repo.config.repo),
      |https| repo.config.git_https = https,
    )
    .await
    .with_context(|| {
      format!(
        "Failed to get git token. Stopping run. | {} | {}",
        repo.config.git_provider, repo.config.git_account
      )
    });
  }
  git_token(
    &stack.config.git_provider,
    &stack.config.git_account,
    Some(&stack.config.repo),
    |https| stack.config.git_https = https,
  )
  .await
  .with_context(|| {
    format!(
      "Failed to get git token. Stopping run. | {} | {}",
      stack.config.git_provider, stack.config.git_account
    )
  })
}

pub async fn build_git_token(
  build: &mut Build,
  repo: Option<&mut Repo>,
) -> anyhow::Result<Option<String>> {
  if let Some(repo) = repo {
    return git_token(
      &repo.config.git_provider,
      &repo.config.git_account,
      Some(&repo.config.repo),
      |https| repo.config.git_https = https,
    )
    .await
    .with_context(|| {
      format!(
        "Failed to get git token. Stopping run. | {} | {}",
        repo.config.git_provider, repo.config.git_account
      )
    });
  }
  git_token(
    &build.config.git_provider,
    &build.config.git_account,
    Some(&build.config.repo),
    |https| build.config.git_https = https,
  )
  .await
  .with_context(|| {
    format!(
      "Failed to get git token. Stopping run. | {} | {}",
      build.config.git_provider, build.config.git_account
    )
  })
}

/// First checks db for token, then checks core config.
/// Only errors if db call errors.
pub async fn registry_token(
  provider_domain: &str,
  account_username: &str,
) -> anyhow::Result<Option<String>> {
  let provider = db_client()
    .registry_accounts
    .find_one(doc! { "domain": provider_domain, "username": account_username })
    .await
    .context("failed to query db for docker registry accounts")?;
  if let Some(provider) = provider {
    // The one place a DB-stored registry token is handed to a caller.
    return Ok(Some(
      crate::crypto::decrypt(&provider.token).with_context(|| {
        format!(
          "Failed to decrypt the registry token for {account_username}@{provider_domain}"
        )
      })?,
    ));
  }
  Ok(
    core_config()
      .image_registries
      .iter()
      .find(|provider| provider.domain == provider_domain)
      .and_then(|provider| {
        provider
          .accounts
          .iter()
          .find(|account| account.username == account_username)
          .map(|account| account.token.clone())
      }),
  )
}

//

pub async fn periphery_client(
  server: &Server,
) -> anyhow::Result<PeripheryClient> {
  if !server.config.enabled {
    return Err(anyhow!("server not enabled"));
  }
  PeripheryClient::new(
    PeripheryConnectionArgs::from_server(server),
    server.config.insecure_tls,
  )
  .await
}

#[instrument(
  "CreatePermission",
  skip(user),
  fields(
    operator = user.id,
    username = user.username
  )
)]
pub async fn create_permission<T>(
  user: &User,
  target: T,
  level: PermissionLevel,
  specific: IndexSet<SpecificPermission>,
) where
  T: Into<ResourceTarget> + std::fmt::Debug,
{
  // No need to actually create permissions for admins
  if user.admin {
    return;
  }
  let target: ResourceTarget = target.into();
  if let Err(e) = db_client()
    .permissions
    .insert_one(Permission {
      id: Default::default(),
      user_target: UserTarget::User(user.id.clone()),
      resource_target: target.clone(),
      level,
      specific,
    })
    .await
  {
    error!("failed to create permission for {target:?} | {e:#}");
  };
}

/// Flattens a document only one level deep
///
/// eg `{ config: { label: "yes", thing: { field1: "ok", field2: "ok" } } }` ->
/// `{ "config.label": "yes", "config.thing": { field1: "ok", field2: "ok" } }`
pub fn flatten_document(doc: Document) -> Document {
  let mut target = Document::new();

  for (outer_field, bson) in doc {
    if let Bson::Document(doc) = bson {
      for (inner_field, bson) in doc {
        target.insert(format!("{outer_field}.{inner_field}"), bson);
      }
    } else {
      target.insert(outer_field, bson);
    }
  }

  target
}

pub fn repo_link(
  provider: &str,
  repo: &str,
  branch: &str,
  https: bool,
) -> String {
  let mut res = format!(
    "http{}://{provider}/{repo}",
    if https { "s" } else { "" }
  );
  // Each provider uses a different link format to get to branches.
  // At least can support github for branch aware link.
  if provider == "github.com" {
    let _ = write!(&mut res, "/tree/{branch}");
  }
  res
}

pub async fn swarm_or_server_request<T>(
  swarm_or_server: &SwarmOrServer,
  request: T,
) -> anyhow::Result<T::Response>
where
  T: std::fmt::Debug + Clone + Serialize + HasResponse,
  T::Response: DeserializeOwned,
{
  match swarm_or_server {
    SwarmOrServer::Swarm(swarm) => {
      swarm_request(&swarm.config.server_ids, request).await
    }
    SwarmOrServer::Server(server) => {
      periphery_client(server).await?.request(request).await
    }
    SwarmOrServer::None => {
      Err(anyhow!("Resource has neither swarm nor server attached."))
    }
  }
}
