use anyhow::Context;
use komodo_client::{
  api::read::*,
  entities::{
    permission::PermissionLevel,
    terraform::{
      Terraform, TerraformActionState, TerraformListItem,
      TerraformSortBy, TerraformState,
    },
  },
};
use mogh_resolver::Resolve;

use crate::{
  helpers::query::get_all_tags, permission::get_check_permissions,
  resource, state::action_states,
};

use super::{ReadArgs, list_limit};

impl Resolve<ReadArgs> for GetTerraform {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Terraform> {
    Ok(
      crate::permission::get_check_permissions_for_read::<Terraform>(
        &self.terraform,
        user,
        PermissionLevel::Read.into(),
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for GetTerraformActionState {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<TerraformActionState> {
    let terraform = get_check_permissions::<Terraform>(
      &self.terraform,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    let action_state = action_states()
      .terraform
      .get(&terraform.id)
      .await
      .unwrap_or_default()
      .get()?;
    Ok(action_state)
  }
}

impl Resolve<ReadArgs> for ListTerraforms {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Vec<TerraformListItem>> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    let limit = list_limit(self.limit);
    let sort_by: resource::ListItemSort<TerraformListItem> =
      match self.sort_by {
        TerraformSortBy::Name => resource::ListItemSort::Name,
        TerraformSortBy::State => {
          resource::ListItemSort::DbField("info.state")
        }
      };
    Ok(
      resource::list_items_for_user::<Terraform>(
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

impl Resolve<ReadArgs> for ListFullTerraforms {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListFullTerraformsResponse> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    let limit = list_limit(self.limit);
    Ok(
      resource::list_full_for_user::<Terraform>(
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

impl Resolve<ReadArgs> for GetTerraformsSummary {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetTerraformsSummaryResponse> {
    let terraforms = resource::list_for_user::<Terraform>(
      Default::default(),
      None,
      None,
      user,
      PermissionLevel::Read.into(),
      &[],
    )
    .await
    .context("failed to get Terraforms from db")?;

    let mut res = GetTerraformsSummaryResponse::default();

    for terraform in terraforms {
      res.total += 1;
      match terraform.info.state {
        TerraformState::Ok => res.ok += 1,
        TerraformState::Drifted => res.drifted += 1,
        TerraformState::Failed => res.failed += 1,
        TerraformState::Unknown => res.unknown += 1,
      }
    }

    Ok(res)
  }
}
