use std::collections::HashMap;

use anyhow::Context;
use database::mungos::{
  by_id::find_one_by_id,
  find::find_collect,
  mongodb::{bson::doc, options::FindOptions},
};
use komodo_client::{
  api::read::{
    GetUpdate, GetUpdateRevertToml, GetUpdateRevertTomlResponse,
    ListUpdates, ListUpdatesResponse,
  },
  entities::{
    permission::PermissionLevel,
    update::{Update, UpdateListItem},
    user::User,
  },
};
use mogh_resolver::Resolve;

use crate::{
  config::core_config,
  permission::{
    check_user_target_access, user_resource_target_query,
  },
  state::db_client,
};

use super::ReadArgs;

const UPDATES_PER_PAGE: i64 = 100;

impl Resolve<ReadArgs> for ListUpdates {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListUpdatesResponse> {
    let query = user_resource_target_query(user, self.query).await?;

    let usernames = find_collect(&db_client().users, None, None)
      .await
      .context("failed to pull users from db")?
      .into_iter()
      .map(|u| (u.id, u.username))
      .collect::<HashMap<_, _>>();

    let updates = find_collect(
      &db_client().updates,
      query,
      FindOptions::builder()
        .sort(doc! { "start_ts": -1 })
        .skip(
          (self.page as u64).saturating_mul(UPDATES_PER_PAGE as u64),
        )
        .limit(UPDATES_PER_PAGE)
        .build(),
    )
    .await
    .context("failed to pull updates from db")?
    .into_iter()
    .map(|u| {
      let username = if User::is_service_user(&u.operator) {
        u.operator.clone()
      } else {
        usernames
          .get(&u.operator)
          .cloned()
          .unwrap_or("unknown".to_string())
      };
      UpdateListItem {
        username,
        id: u.id,
        operation: u.operation,
        start_ts: u.start_ts,
        success: u.success,
        operator: u.operator,
        target: u.target,
        status: u.status,
        version: u.version,
        other_data: u.other_data,
      }
    })
    .collect::<Vec<_>>();

    let next_page = if updates.len() == UPDATES_PER_PAGE as usize {
      Some(self.page + 1)
    } else {
      None
    };

    Ok(ListUpdatesResponse { updates, next_page })
  }
}

impl Resolve<ReadArgs> for GetUpdate {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Update> {
    let update = find_one_by_id(&db_client().updates, &self.id)
      .await
      .context("failed to query to db")?
      .context("no update exists with given id")?;
    if user.admin || core_config().transparent_mode {
      return Ok(update);
    }
    check_user_target_access(
      &update.target,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    Ok(update)
  }
}

impl Resolve<ReadArgs> for GetUpdateRevertToml {
  async fn resolve(
    self,
    args: &ReadArgs,
  ) -> mogh_error::Result<GetUpdateRevertTomlResponse> {
    // Same permission path as reading the Update itself - this returns
    // a subset of it, so it must not be easier to reach.
    let update = GetUpdate {
      id: self.update.clone(),
    }
    .resolve(args)
    .await?;

    let prev = update.prev_toml.trim();
    if prev.is_empty() {
      // Not every Update changes config. Applying an empty TOML would
      // be read as "this sync manages nothing", which for a managed
      // sync means DELETE EVERYTHING - so this refusal is the single
      // most important line in the feature.
      return Ok(GetUpdateRevertTomlResponse {
        revertable: false,
        reason: String::from(
          "This update recorded no config snapshot, so there is nothing to revert to. Only updates that CHANGED a resource's config carry one.",
        ),
        toml: String::new(),
        current_toml: update.current_toml,
      });
    }

    // Parse before offering it. A snapshot that no longer deserializes -
    // written by an older Komodo whose schema has since changed - would
    // otherwise fail deep inside a sync run, after the operator had
    // already confirmed.
    if let Err(e) = crate::sync::deserialize_resources_toml(prev) {
      return Ok(GetUpdateRevertTomlResponse {
        revertable: false,
        reason: format!(
          "The stored snapshot no longer parses, most likely written by an older Komodo whose config schema has since changed: {e:#}"
        ),
        toml: String::new(),
        current_toml: update.current_toml,
      });
    }

    Ok(GetUpdateRevertTomlResponse {
      revertable: true,
      reason: String::new(),
      toml: prev.to_string(),
      current_toml: update.current_toml,
    })
  }
}
