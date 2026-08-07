use anyhow::{Context, anyhow};
use database::mongo_indexed::{Document, doc};
use database::mungos::{
  by_id::find_one_by_id, find::find_collect,
  mongodb::options::FindOptions,
};
use komodo_client::api::read::*;
use komodo_client::entities::provider::{
  GitProviderAccount, ImageRegistryAccount,
};
use mogh_resolver::Resolve;

use crate::{resource::redacted, state::db_client};

use super::ReadArgs;

impl Resolve<ReadArgs> for GetGitProviderAccount {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetGitProviderAccountResponse> {
    if !user.admin {
      return Err(
        anyhow!("Only admins can read git provider accounts").into(),
      );
    }
    let res = find_one_by_id(&db_client().git_accounts, &self.id)
      .await
      .context("failed to query db for git provider accounts")?
      .context(
        "did not find git provider account with the given id",
      )?;
    Ok(redact_git_token(res))
  }
}

impl Resolve<ReadArgs> for ListGitProviderAccounts {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListGitProviderAccountsResponse> {
    if !user.admin {
      return Err(
        anyhow!("Only admins can read git provider accounts").into(),
      );
    }
    let mut filter = Document::new();
    if let Some(domain) = self.domain {
      filter.insert("domain", domain);
    }
    if let Some(username) = self.username {
      filter.insert("username", username);
    }
    let res = find_collect(
      &db_client().git_accounts,
      filter,
      FindOptions::builder()
        .sort(doc! { "domain": 1, "username": 1 })
        .build(),
    )
    .await
    .context("failed to query db for git provider accounts")?;
    Ok(res.into_iter().map(redact_git_token).collect())
  }
}

impl Resolve<ReadArgs> for GetImageRegistryAccount {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetImageRegistryAccountResponse> {
    if !user.admin {
      return Err(
        anyhow!("Only admins can read docker registry accounts")
          .into(),
      );
    }
    let res =
      find_one_by_id(&db_client().registry_accounts, &self.id)
        .await
        .context("failed to query db for docker registry accounts")?
        .context(
          "did not find docker registry account with the given id",
        )?;
    Ok(redact_registry_token(res))
  }
}

impl Resolve<ReadArgs> for ListImageRegistryAccounts {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListImageRegistryAccountsResponse> {
    if !user.admin {
      return Err(
        anyhow!("Only admins can read docker registry accounts")
          .into(),
      );
    }
    let mut filter = Document::new();
    if let Some(domain) = self.domain {
      filter.insert("domain", domain);
    }
    if let Some(username) = self.username {
      filter.insert("username", username);
    }
    let res = find_collect(
      &db_client().registry_accounts,
      filter,
      FindOptions::builder()
        .sort(doc! { "domain": 1, "username": 1 })
        .build(),
    )
    .await
    .context("failed to query db for docker registry accounts")?;
    Ok(res.into_iter().map(redact_registry_token).collect())
  }
}

/// Strip the token from a git provider account before it leaves Core.
///
/// These reads are already admin-only, so this closes no privilege
/// escalation - it just stops a live credential sitting in an admin's
/// browser memory, devtools and any log that captures the response.
/// The config-file twin (ProviderAccount) has carried an unconditional
/// skip_serializing since forever; this brings the DB-backed ones in line.
fn redact_git_token(
  mut account: GitProviderAccount,
) -> GitProviderAccount {
  account.token = redacted(&account.token);
  account
}

/// See [redact_git_token].
fn redact_registry_token(
  mut account: ImageRegistryAccount,
) -> ImageRegistryAccount {
  account.token = redacted(&account.token);
  account
}
