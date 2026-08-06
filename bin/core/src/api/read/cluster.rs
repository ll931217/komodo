use anyhow::{Context, anyhow};
use komodo_client::{
  api::read::*,
  entities::{
    cluster::{
      Cluster, ClusterActionState, ClusterListItem,
      ClusterMetricsKind, ClusterSortBy, ClusterState,
      is_cluster_scoped_kind,
    },
    permission::PermissionLevel,
    server::Server,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  GetClusterPodLog as PeripheryGetClusterPodLog,
  GetClusterPodLogSearch, GetClusterResources, GetClusterTop,
  InspectHelmRelease as PeripheryInspectHelmRelease,
  ListHelmReleases as PeripheryListHelmReleases,
};

use crate::{
  helpers::{
    cluster::cluster_target, periphery_client, query::get_all_tags,
  },
  permission::get_check_permissions,
  resource,
  state::action_states,
};

use super::{ReadArgs, list_limit};

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
    let limit = list_limit(self.limit);
    let sort_by: resource::ListItemSort<ClusterListItem> =
      match self.sort_by {
        ClusterSortBy::Name => resource::ListItemSort::Name,
        ClusterSortBy::State => {
          resource::ListItemSort::InMemory(Box::new(|a, b| {
            a.info
              .state
              .cmp(&b.info.state)
              .then_with(|| a.name.cmp(&b.name))
          }))
        }
      };
    Ok(
      resource::list_items_for_user::<Cluster>(
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
    let limit = list_limit(self.limit);
    Ok(
      resource::list_full_for_user::<Cluster>(
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
      None,
      None,
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

/// Resolve which namespace a read targets, enforcing the Cluster's
/// scoping controls. Reads are gated the same way executions are: if a
/// Cluster may not touch cluster-scoped objects, it may not enumerate
/// them either.
async fn resolve_scope(
  cluster: &str,
  kind: &str,
  namespace: Option<String>,
  all_namespaces: bool,
  user: &komodo_client::entities::user::User,
) -> anyhow::Result<(Cluster, String, bool)> {
  let cluster = get_check_permissions::<Cluster>(
    cluster,
    user,
    PermissionLevel::Read.inspect(),
  )
  .await?;

  if is_cluster_scoped_kind(kind) && !cluster.config.cluster_resources
  {
    anyhow::bail!(
      "Kind '{kind}' is cluster-scoped, but this Cluster has cluster resources disabled"
    );
  }

  if all_namespaces && !cluster.config.namespaces.is_empty() {
    anyhow::bail!(
      "This Cluster restricts namespaces to {:?}, so reading across all namespaces is not allowed",
      cluster.config.namespaces
    );
  }

  let namespace = match namespace {
    Some(namespace) if !namespace.is_empty() => namespace,
    _ => cluster.config.default_namespace().to_string(),
  };
  if !cluster.config.namespace_allowed(&namespace) {
    anyhow::bail!(
      "Namespace '{namespace}' is not in this Cluster's allowed namespaces {:?}",
      cluster.config.namespaces
    );
  }

  Ok((cluster, namespace, all_namespaces))
}

impl Resolve<ReadArgs> for ListClusterResources {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListClusterResourcesResponse> {
    let (cluster, namespace, all_namespaces) = resolve_scope(
      &self.cluster,
      &self.kind,
      self.namespace,
      self.all_namespaces,
      user,
    )
    .await?;
    Ok(
      get_resources(
        &cluster,
        &self.kind,
        namespace,
        None,
        all_namespaces,
      )
      .await?,
    )
  }
}

impl Resolve<ReadArgs> for GetClusterMetrics {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetClusterMetricsResponse> {
    // The same scoping rules as listing the kind itself:
    // node metrics are a cluster-scoped read, pod metrics respect
    // the namespace restrictions.
    let kind = match self.kind {
      ClusterMetricsKind::Nodes => "nodes",
      ClusterMetricsKind::Pods => "pods",
    };
    let (cluster, namespace, all_namespaces) = resolve_scope(
      &self.cluster,
      kind,
      self.namespace,
      self.all_namespaces,
      user,
    )
    .await?;
    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;
    Ok(
      periphery_client(&server)
        .await?
        .request(GetClusterTop {
          target: cluster_target(&cluster).await?,
          kind: self.kind,
          namespace,
          all_namespaces,
        })
        .await?,
    )
  }
}

impl Resolve<ReadArgs> for ListHelmReleases {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<ListHelmReleasesResponse> {
    // Releases are namespaced, so the "helm" kind never trips the
    // cluster-scoped gate and the namespace rules apply as usual.
    let (cluster, namespace, all_namespaces) = resolve_scope(
      &self.cluster,
      "helm",
      self.namespace,
      self.all_namespaces,
      user,
    )
    .await?;
    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;
    Ok(
      periphery_client(&server)
        .await?
        .request(PeripheryListHelmReleases {
          target: cluster_target(&cluster).await?,
          namespace,
          all_namespaces,
        })
        .await?,
    )
  }
}

impl Resolve<ReadArgs> for InspectHelmRelease {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<InspectHelmReleaseResponse> {
    let (cluster, namespace, _) = resolve_scope(
      &self.cluster,
      "helm",
      self.namespace,
      false,
      user,
    )
    .await?;
    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;
    Ok(
      periphery_client(&server)
        .await?
        .request(PeripheryInspectHelmRelease {
          target: cluster_target(&cluster).await?,
          name: self.name,
          namespace,
        })
        .await?,
    )
  }
}

impl Resolve<ReadArgs> for InspectClusterResource {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<InspectClusterResourceResponse> {
    let (cluster, namespace, _) = resolve_scope(
      &self.cluster,
      &self.kind,
      self.namespace,
      false,
      user,
    )
    .await?;
    Ok(
      get_resources(
        &cluster,
        &self.kind,
        namespace,
        Some(self.name),
        false,
      )
      .await?,
    )
  }
}

async fn get_resources(
  cluster: &Cluster,
  kind: &str,
  namespace: String,
  name: Option<String>,
  all_namespaces: bool,
) -> anyhow::Result<serde_json::Value> {
  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;
  periphery_client(&server)
    .await?
    .request(GetClusterResources {
      target: cluster_target(cluster).await?,
      kind: kind.to_string(),
      namespace,
      name,
      all_namespaces,
    })
    .await
}

impl Resolve<ReadArgs> for GetClusterPodLog {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<GetClusterPodLogResponse> {
    // Reading logs needs the Logs specific permission, matching how
    // container and stack logs are gated.
    let cluster = get_check_permissions::<Cluster>(
      &self.cluster,
      user,
      PermissionLevel::Read.logs(),
    )
    .await?;

    let namespace = match self.namespace {
      Some(namespace) if !namespace.is_empty() => namespace,
      _ => cluster.config.default_namespace().to_string(),
    };
    if !cluster.config.namespace_allowed(&namespace) {
      return Err(
        anyhow!(
          "Namespace '{namespace}' is not in this Cluster's allowed namespaces {:?}",
          cluster.config.namespaces
        )
        .into(),
      );
    }

    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;

    Ok(
      periphery_client(&server)
        .await?
        .request(PeripheryGetClusterPodLog {
          target: cluster_target(&cluster).await?,
          namespace,
          pod: self.pod,
          container: self.container,
          tail: self.tail.unwrap_or(100),
          previous: self.previous,
          timestamps: self.timestamps,
        })
        .await?,
    )
  }
}

impl Resolve<ReadArgs> for SearchClusterPodLog {
  async fn resolve(
    self,
    ReadArgs { user }: &ReadArgs,
  ) -> mogh_error::Result<SearchClusterPodLogResponse> {
    // Same Logs permission gate as GetClusterPodLog.
    let cluster = get_check_permissions::<Cluster>(
      &self.cluster,
      user,
      PermissionLevel::Read.logs(),
    )
    .await?;

    let namespace = match self.namespace {
      Some(namespace) if !namespace.is_empty() => namespace,
      _ => cluster.config.default_namespace().to_string(),
    };
    if !cluster.config.namespace_allowed(&namespace) {
      return Err(
        anyhow!(
          "Namespace '{namespace}' is not in this Cluster's allowed namespaces {:?}",
          cluster.config.namespaces
        )
        .into(),
      );
    }

    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;

    Ok(
      periphery_client(&server)
        .await?
        .request(GetClusterPodLogSearch {
          target: cluster_target(&cluster).await?,
          namespace,
          pod: self.pod,
          container: self.container,
          terms: self.terms,
          combinator: self.combinator,
          invert: self.invert,
          timestamps: self.timestamps,
        })
        .await?,
    )
  }
}
