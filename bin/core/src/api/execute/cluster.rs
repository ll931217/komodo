use anyhow::{Context, anyhow};
use formatting::format_serror;
use interpolate::Interpolator;
use komodo_client::{
  api::execute::*,
  entities::{
    cluster::{
      Cluster, ClusterManifestSourceKind, is_cluster_scoped_kind,
    },
    permission::PermissionLevel,
    server::Server,
    update::Update,
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ApplyClusterManifests, ClusterApplyMode, DeleteClusterResource,
};

use crate::{
  helpers::{
    cluster::{
      InterpolatedCluster, cluster_manifest_source, cluster_target,
      interpolated_cluster,
    },
    periphery_client,
    update::update_update,
  },
  permission::get_check_permissions,
  resource,
};

use super::{BatchExecutionResponse, ExecuteArgs, ExecuteRequest};

impl super::BatchExecute for BatchDeployCluster {
  type Resource = Cluster;
  fn single_request(cluster: String) -> ExecuteRequest {
    ExecuteRequest::DeployCluster(DeployCluster {
      cluster,
      namespace: None,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDeployCluster {
  #[instrument(
    "BatchDeployCluster",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchDeployCluster>(&self.pattern, user)
        .await?,
    )
  }
}

impl super::BatchExecute for BatchDestroyCluster {
  type Resource = Cluster;
  fn single_request(cluster: String) -> ExecuteRequest {
    ExecuteRequest::DestroyCluster(DestroyCluster {
      cluster,
      namespace: None,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDestroyCluster {
  #[instrument(
    "BatchDestroyCluster",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchDestroyCluster>(
        &self.pattern,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DeployCluster {
  #[instrument(
    "DeployCluster",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(
      execute_manifests(
        &self.cluster,
        self.namespace,
        ClusterApplyMode::Apply,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DestroyCluster {
  #[instrument(
    "DestroyCluster",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(
      execute_manifests(
        &self.cluster,
        self.namespace,
        ClusterApplyMode::Delete,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DiffCluster {
  #[instrument(
    "DiffCluster",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    Ok(
      execute_manifests(
        &self.cluster,
        self.namespace,
        ClusterApplyMode::Diff,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DeleteClusterObject {
  #[instrument(
    "DeleteClusterObject",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      kind = self.kind,
      name = self.name,
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs {
      user,
      update,
      task_id,
    }: &ExecuteArgs,
  ) -> mogh_error::Result<Update> {
    let mut update = update.clone();
    let cluster = get_check_permissions::<Cluster>(
      &self.cluster,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    // Deleting a live object is gated by the same scoping controls as
    // applying manifests that declare one.
    if is_cluster_scoped_kind(&self.kind)
      && !cluster.config.cluster_resources
    {
      return Err(
        anyhow!(
          "Kind '{}' is cluster-scoped, but this Cluster has cluster resources disabled",
          self.kind
        )
        .into(),
      );
    }
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

    match periphery_client(&server)
      .await?
      .request(DeleteClusterResource {
        target: cluster_target(&cluster).await?,
        kind: self.kind,
        namespace,
        name: self.name,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update
        .push_error_log("Delete Object", format_serror(&e.into())),
    }

    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

fn stage(mode: ClusterApplyMode) -> &'static str {
  match mode {
    ClusterApplyMode::Apply => "Deploy",
    ClusterApplyMode::Delete => "Destroy",
    ClusterApplyMode::Diff => "Diff",
  }
}

/// Shared apply / delete / diff path.
///
/// Enforces the Cluster's scoping controls before anything reaches the
/// cluster: an execution may only target a permitted namespace, and
/// manifests declaring cluster-scoped objects are rejected outright
/// when `cluster_resources` is off.
async fn execute_manifests(
  cluster: &str,
  namespace_override: Option<String>,
  mode: ClusterApplyMode,
  user: &User,
  mut update: Update,
) -> anyhow::Result<Update> {
  let cluster = get_check_permissions::<Cluster>(
    cluster,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;

  // Only the Contents source needs file_contents; the others read
  // from the host or a repo, where an empty field is expected.
  if cluster.config.manifest_source()
    == ClusterManifestSourceKind::Contents
    && cluster.config.file_contents.trim().is_empty()
  {
    return Err(anyhow!("Cluster has no manifests configured"));
  }

  let namespace = match namespace_override {
    Some(namespace) if !namespace.is_empty() => namespace,
    _ => cluster.config.default_namespace().to_string(),
  };
  if !cluster.config.namespace_allowed(&namespace) {
    return Err(anyhow!(
      "Namespace '{namespace}' is not in this Cluster's allowed namespaces {:?}",
      cluster.config.namespaces
    ));
  }

  if !cluster.config.cluster_resources
    && let Some(kind) =
      cluster_scoped_kind(&cluster.config.file_contents)
  {
    return Err(anyhow!(
      "Manifests declare cluster-scoped kind '{kind}', but this Cluster has cluster resources disabled"
    ));
  }

  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;

  let InterpolatedCluster {
    target,
    manifests,
    secret_replacers,
  } = interpolated_cluster(&cluster).await?;
  let source = cluster_manifest_source(&cluster, manifests).await?;

  let res = match periphery_client(&server)
    .await?
    .request(ApplyClusterManifests {
      target,
      source,
      namespace,
      kustomize: cluster.config.kustomize,
      mode,
      extra_args: cluster.config.extra_args.clone(),
      secret_replacers,
    })
    .await
  {
    Ok(res) => res,
    Err(e) => {
      update.push_error_log(stage(mode), format_serror(&e.into()));
      update.finalize();
      update_update(update.clone()).await?;
      return Ok(update);
    }
  };

  update.logs.extend(res.logs);
  // Record what was deployed, for repo sources.
  if let Some(hash) = res.commit_hash {
    update.commit_hash = hash;
  }
  update.finalize();
  update_update(update.clone()).await?;

  Ok(update)
}

/// The first cluster-scoped kind declared in `manifests`, if any.
///
/// A deliberately shallow scan of `kind:` lines rather than a full
/// parse: it errs toward refusing, which is the safe direction for a
/// blast-radius control.
fn cluster_scoped_kind(manifests: &str) -> Option<String> {
  manifests.lines().find_map(|line| {
    let kind =
      line.trim().strip_prefix("kind:")?.trim().trim_matches('"');
    is_cluster_scoped_kind(kind).then(|| kind.to_string())
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detects_cluster_scoped_kinds() {
    let manifests =
      "apiVersion: v1\nkind: Namespace\nmetadata:\n  name: foo\n";
    assert_eq!(
      cluster_scoped_kind(manifests),
      Some("Namespace".to_string())
    );

    // Namespaced kinds are allowed through.
    let manifests = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: foo\n";
    assert_eq!(cluster_scoped_kind(manifests), None);

    // Must not match a kind appearing as a value elsewhere.
    let manifests = "metadata:\n  labels:\n    app: ClusterRole\n";
    assert_eq!(cluster_scoped_kind(manifests), None);

    // Finds it in a multi-document manifest.
    let manifests = "kind: Deployment\n---\nkind: ClusterRole\n";
    assert_eq!(
      cluster_scoped_kind(manifests),
      Some("ClusterRole".to_string())
    );
  }
}
