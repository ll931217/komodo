use anyhow::Context;
use komodo_client::{
  api::read::*,
  entities::{
    application::{
      Application, ApplicationActionState, ApplicationListItem,
      ApplicationSortBy, ApplicationState,
    },
    permission::PermissionLevel,
  },
};
use mogh_resolver::Resolve;

use crate::{
  helpers::query::get_all_tags, permission::get_check_permissions,
  resource, state::action_states,
};

use super::{ReadArgs, list_limit};

impl Resolve<ReadArgs> for GetApplication {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Application> {
    Ok(
      crate::permission::get_check_permissions_for_read::<Application>(
        &self.application,
        user,
        PermissionLevel::Read.into(),
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for GetApplicationActionState {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ApplicationActionState> {
    let application = get_check_permissions::<Application>(
      &self.application,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    let action_state = action_states()
      .application
      .get(&application.id)
      .await
      .unwrap_or_default()
      .get()?;
    Ok(action_state)
  }
}

impl Resolve<ReadArgs> for ListApplications {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Vec<ApplicationListItem>> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    let limit = list_limit(self.limit);
    let sort_by: resource::ListItemSort<ApplicationListItem> =
      match self.sort_by {
        ApplicationSortBy::Name => resource::ListItemSort::Name,
        ApplicationSortBy::State => {
          resource::ListItemSort::DbField("info.state")
        }
      };
    Ok(
      resource::list_items_for_user::<Application>(
        self.query,
        resource::ListItemsQueryOptions {
          limit,
          page: self.page,
          sort_desc: self.sort_desc,
          sort_by,
        },
        user,
        PermissionLevel::Read.into(),
        &all_tags,
        |_| true,
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for ListFullApplications {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListFullApplicationsResponse> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    let limit = list_limit(self.limit);
    Ok(
      resource::list_full_for_user::<Application>(
        self.query,
        limit as i64,
        self.page.saturating_mul(limit),
        user,
        PermissionLevel::Read.into(),
        &all_tags,
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for GetApplicationsSummary {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetApplicationsSummaryResponse> {
    let applications = resource::list_for_user::<Application>(
      Default::default(),
      None,
      None,
      user,
      PermissionLevel::Read.into(),
      &[],
    )
    .await
    .context("failed to get Applications from db")?;

    let mut res = GetApplicationsSummaryResponse::default();

    for application in applications {
      res.total += 1;
      match application.info.state {
        ApplicationState::Deployed => res.deployed += 1,
        ApplicationState::Drifted => res.drifted += 1,
        ApplicationState::Failed => res.failed += 1,
        ApplicationState::Unknown => res.unknown += 1,
      }
    }

    Ok(res)
  }
}
