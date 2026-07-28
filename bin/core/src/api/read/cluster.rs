use anyhow::Context;
use komodo_client::{
  api::read::*,
  entities::{
    cluster::{
      Cluster, ClusterActionState, ClusterListItem, ClusterState,
    },
    permission::PermissionLevel,
  },
};
use mogh_resolver::Resolve;

use crate::{
  helpers::query::get_all_tags, permission::get_check_permissions,
  resource, state::action_states,
};

use super::ReadArgs;

impl Resolve<ReadArgs> for GetCluster {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Cluster> {
    Ok(
      get_check_permissions::<Cluster>(
        &self.cluster,
        user,
        PermissionLevel::Read.into(),
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for ListClusters {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<Vec<ClusterListItem>> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    Ok(
      resource::list_for_user::<Cluster>(
        self.query,
        user,
        PermissionLevel::Read.into(),
        &all_tags,
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for ListFullClusters {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListFullClustersResponse> {
    let all_tags = if self.query.tags.is_empty() {
      vec![]
    } else {
      get_all_tags(None).await?
    };
    Ok(
      resource::list_full_for_user::<Cluster>(
        self.query,
        user,
        PermissionLevel::Read.into(),
        &all_tags,
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for GetClusterActionState {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ClusterActionState> {
    let cluster = get_check_permissions::<Cluster>(
      &self.cluster,
      user,
      PermissionLevel::Read.into(),
    )
    .await?;
    let action_state = action_states()
      .cluster
      .get(&cluster.id)
      .await
      .unwrap_or_default()
      .get()?;
    Ok(action_state)
  }
}

impl Resolve<ReadArgs> for GetClustersSummary {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetClustersSummaryResponse> {
    let clusters = resource::list_for_user::<Cluster>(
      Default::default(),
      user,
      PermissionLevel::Read.into(),
      &[],
    )
    .await
    .context("failed to get clusters from db")?;

    let mut res = GetClustersSummaryResponse::default();

    for cluster in clusters {
      res.total += 1;
      match cluster.info.state {
        ClusterState::Ok => res.ok += 1,
        ClusterState::Unreachable => res.unreachable += 1,
        ClusterState::Unknown => res.unknown += 1,
      }
    }

    Ok(res)
  }
}
