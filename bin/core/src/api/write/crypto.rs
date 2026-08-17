use anyhow::{Context, anyhow};
use database::mungos::{
  find::find_collect,
  mongodb::bson::{doc, oid::ObjectId},
};
use komodo_client::api::write::{
  ReencryptSecrets, ReencryptSecretsResponse,
};
use mogh_error::AddStatusCodeError as _;
use mogh_resolver::Resolve;
use reqwest::StatusCode;

use crate::{crypto, state::db_client};

use super::WriteArgs;

/// One value's worth of the pass: what is stored now, and what should
/// replace it.
///
/// Returns `Ok(None)` when the value is already written by the newest
/// key and should be left alone - that is what makes running this
/// twice a no-op rather than a database-wide rewrite with fresh
/// nonces.
fn reencrypted(
  stored: &str,
  newest: u32,
) -> anyhow::Result<Option<String>> {
  if crypto::key_version(stored) == Some(newest) {
    return Ok(None);
  }
  // Plaintext passes through decrypt unchanged, so this one path
  // covers both rotating an old key forward and encrypting a value
  // that predates encryption being switched on.
  let plaintext = crypto::decrypt(stored)?;
  Ok(Some(crypto::encrypt(&plaintext)?))
}

impl Resolve<WriteArgs> for ReencryptSecrets {
  #[instrument(
    "ReencryptSecrets",
    skip_all,
    fields(operator = user.id, dry_run = self.dry_run)
  )]
  async fn resolve(
    self,
    WriteArgs { user }: &WriteArgs,
  ) -> mogh_error::Result<ReencryptSecretsResponse> {
    if !user.admin {
      return Err(
        anyhow!("Only Admins can re-encrypt secrets")
          .status_code(StatusCode::FORBIDDEN),
      );
    }

    // Refusing here rather than proceeding matters: with no key
    // configured, encrypt() is a no-op, so the pass would report
    // having rewritten every secret while writing them all back as
    // plaintext.
    let Some(newest) = crypto::newest_version()? else {
      return Err(
        anyhow!(
          "No 'secret_keys' configured, so there is nothing to \
           encrypt with. Set a key on Core before running this."
        )
        .status_code(StatusCode::BAD_REQUEST),
      );
    };

    let db = db_client();
    let mut res = ReencryptSecretsResponse {
      dry_run: self.dry_run,
      ..Default::default()
    };

    let variables = find_collect(&db.variables, None, None)
      .await
      .context("Failed to query db for variables")?;
    for variable in variables {
      if !variable.is_secret {
        continue;
      }
      match reencrypted(&variable.value, newest) {
        Ok(None) => res.already_current += 1,
        Ok(Some(value)) => {
          if !self.dry_run {
            db.variables
              .update_one(
                doc! { "name": &variable.name },
                doc! { "$set": { "value": value } },
              )
              .await
              .with_context(|| {
                format!(
                  "Failed to rewrite variable {}",
                  variable.name
                )
              })?;
          }
          res.variables += 1;
        }
        // Reported, not written. A value this pass cannot read is
        // the one value it must not replace.
        Err(e) => res
          .failed
          .push(format!("Variable {}: {e:#}", variable.name)),
      }
    }

    let git_accounts = find_collect(&db.git_accounts, None, None)
      .await
      .context("Failed to query db for git provider accounts")?;
    for account in git_accounts {
      match reencrypted(&account.token, newest) {
        Ok(None) => res.already_current += 1,
        Ok(Some(token)) => {
          if !self.dry_run {
            db.git_accounts
              .update_one(
                doc! { "_id": ObjectId::parse_str(&account.id).context("Bad git account id")? },
                doc! { "$set": { "token": token } },
              )
              .await
              .with_context(|| {
                format!(
                  "Failed to rewrite git account {}/{}",
                  account.domain, account.username
                )
              })?;
          }
          res.git_accounts += 1;
        }
        Err(e) => res.failed.push(format!(
          "Git account {}/{}: {e:#}",
          account.domain, account.username
        )),
      }
    }

    let registry_accounts =
      find_collect(&db.registry_accounts, None, None)
        .await
        .context("Failed to query db for registry accounts")?;
    for account in registry_accounts {
      match reencrypted(&account.token, newest) {
        Ok(None) => res.already_current += 1,
        Ok(Some(token)) => {
          if !self.dry_run {
            db.registry_accounts
              .update_one(
                doc! { "_id": ObjectId::parse_str(&account.id).context("Bad registry account id")? },
                doc! { "$set": { "token": token } },
              )
              .await
              .with_context(|| {
                format!(
                  "Failed to rewrite registry account {}/{}",
                  account.domain, account.username
                )
              })?;
          }
          res.registry_accounts += 1;
        }
        Err(e) => res.failed.push(format!(
          "Registry account {}/{}: {e:#}",
          account.domain, account.username
        )),
      }
    }

    Ok(res)
  }
}
