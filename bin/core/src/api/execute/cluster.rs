use anyhow::{Context, anyhow};
use formatting::format_serror;
use interpolate::Interpolator;
use komodo_client::{
  api::execute::*,
  entities::{
    cluster::{Cluster, ClusterConfig},
    permission::PermissionLevel,
    server::Server,
    update::Update,
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ApplyClusterManifests, ClusterApplyMode, ClusterTarget,
};

use crate::{
  helpers::{
    periphery_client,
    query::{VariablesAndSecrets, get_variables_and_secrets},
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

  if cluster.config.file_contents.trim().is_empty() {
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

  let (kubeconfig_contents, manifests, secret_replacers) =
    interpolate(&cluster.config).await?;

  let logs = match periphery_client(&server)
    .await?
    .request(ApplyClusterManifests {
      target: ClusterTarget {
        kubeconfig_contents,
        kubeconfig_path: cluster.config.kubeconfig_path.clone(),
        context: cluster.config.context.clone(),
        proxy_url: cluster.config.proxy_url.clone(),
      },
      manifests,
      namespace,
      kustomize: cluster.config.kustomize,
      mode,
      extra_args: cluster.config.extra_args.clone(),
      secret_replacers,
    })
    .await
  {
    Ok(logs) => logs,
    Err(e) => {
      update.push_error_log(stage(mode), format_serror(&e.into()));
      update.finalize();
      update_update(update.clone()).await?;
      return Ok(update);
    }
  };

  update.logs.extend(logs);
  update.finalize();
  update_update(update.clone()).await?;

  Ok(update)
}

/// Interpolate Variables / secrets into the kubeconfig and manifests,
/// returning the replacers so Periphery can scrub secret values out of
/// the command output before it lands in the Update log.
async fn interpolate(
  config: &ClusterConfig,
) -> anyhow::Result<(String, String, Vec<(String, String)>)> {
  let mut kubeconfig_contents = config.kubeconfig_contents.clone();
  let mut manifests = config.file_contents.clone();

  if config.skip_secret_interp {
    return Ok((kubeconfig_contents, manifests, Vec::new()));
  }

  let VariablesAndSecrets { variables, secrets } =
    get_variables_and_secrets()
      .await
      .context("Failed to get variables and secrets")?;
  let mut interpolator =
    Interpolator::new(Some(&variables), &secrets);
  interpolator
    .interpolate_string(&mut kubeconfig_contents)
    .context("Failed to interpolate variables into kubeconfig")?
    .interpolate_string(&mut manifests)
    .context("Failed to interpolate variables into manifests")?;

  Ok((
    kubeconfig_contents,
    manifests,
    interpolator.secret_replacers.into_iter().collect(),
  ))
}

/// Cluster-scoped kinds Komodo refuses to touch when a Cluster has
/// `cluster_resources` disabled.
///
/// This is a deliberately shallow scan of `kind:` lines rather than a
/// full parse: it errs toward refusing, which is the safe direction for
/// a blast-radius control, and it does not need to understand every CRD.
fn cluster_scoped_kind(manifests: &str) -> Option<String> {
  const CLUSTER_SCOPED: &[&str] = &[
    "Namespace",
    "Node",
    "PersistentVolume",
    "ClusterRole",
    "ClusterRoleBinding",
    "CustomResourceDefinition",
    "StorageClass",
    "IngressClass",
    "PriorityClass",
    "RuntimeClass",
    "MutatingWebhookConfiguration",
    "ValidatingWebhookConfiguration",
    "APIService",
    "CSIDriver",
    "CSINode",
  ];
  manifests.lines().find_map(|line| {
    let line = line.trim();
    let kind = line.strip_prefix("kind:")?.trim().trim_matches('"');
    CLUSTER_SCOPED.contains(&kind).then(|| kind.to_string())
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
