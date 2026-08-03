use anyhow::{Context, anyhow};
use formatting::format_serror;
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
  ApplyClusterManifests,
  ApplyClusterObject as PeripheryApplyClusterObject,
  ClusterApplyMode, ClusterRolloutVerb, DeleteClusterResource,
  DrainClusterNode as PeripheryDrainClusterNode,
  RolloutClusterWorkload, ScaleClusterResource,
  SetClusterNodeSchedulable,
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

impl Resolve<ExecuteArgs> for RestartClusterWorkload {
  #[instrument(
    "RestartClusterWorkload",
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
    check_workload_kind(&self.kind, ROLLOUT_KINDS, "rollout")?;
    Ok(
      rollout_workload(
        &self.cluster,
        ClusterRolloutVerb::Restart,
        self.kind,
        self.name,
        self.namespace,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for RollbackClusterWorkload {
  #[instrument(
    "RollbackClusterWorkload",
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
    check_workload_kind(&self.kind, ROLLOUT_KINDS, "rollout")?;
    Ok(
      rollout_workload(
        &self.cluster,
        ClusterRolloutVerb::Undo,
        self.kind,
        self.name,
        self.namespace,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

async fn rollout_workload(
  cluster: &str,
  verb: ClusterRolloutVerb,
  kind: String,
  name: String,
  namespace: Option<String>,
  user: &User,
  mut update: Update,
) -> anyhow::Result<Update> {
  let (cluster, namespace, server) = cluster_execution_setup(
    cluster,
    namespace,
    user,
    PermissionLevel::Execute,
  )
  .await?;

  match periphery_client(&server)
    .await?
    .request(RolloutClusterWorkload {
      target: cluster_target(&cluster).await?,
      verb,
      kind,
      name,
      namespace,
    })
    .await
  {
    Ok(log) => update.logs.push(log),
    Err(e) => {
      update.push_error_log("Rollout", format_serror(&e.into()))
    }
  }

  update.finalize();
  update_update(update.clone()).await?;
  Ok(update)
}

impl Resolve<ExecuteArgs> for ScaleClusterWorkload {
  #[instrument(
    "ScaleClusterWorkload",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      kind = self.kind,
      name = self.name,
      replicas = self.replicas,
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
    check_workload_kind(&self.kind, SCALE_KINDS, "scale")?;
    let (cluster, namespace, server) = cluster_execution_setup(
      &self.cluster,
      self.namespace,
      user,
      PermissionLevel::Execute,
    )
    .await?;

    match periphery_client(&server)
      .await?
      .request(ScaleClusterResource {
        target: cluster_target(&cluster).await?,
        kind: self.kind,
        name: self.name,
        replicas: self.replicas,
        namespace,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => {
        update.push_error_log("Scale", format_serror(&e.into()))
      }
    }

    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for CordonClusterNode {
  #[instrument(
    "CordonClusterNode",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      node = self.node,
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
      set_node_schedulable(
        &self.cluster,
        self.node,
        false,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for UncordonClusterNode {
  #[instrument(
    "UncordonClusterNode",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      node = self.node,
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
      set_node_schedulable(
        &self.cluster,
        self.node,
        true,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

/// Nodes are cluster-scoped, so node operations follow the same gate
/// as touching any other cluster-scoped object.
fn check_node_ops_allowed(cluster: &Cluster) -> anyhow::Result<()> {
  if cluster.config.cluster_resources {
    Ok(())
  } else {
    Err(anyhow!(
      "Node operations are cluster-scoped, but this Cluster has cluster resources disabled"
    ))
  }
}

async fn set_node_schedulable(
  cluster: &str,
  node: String,
  schedulable: bool,
  user: &User,
  mut update: Update,
) -> anyhow::Result<Update> {
  let cluster = get_check_permissions::<Cluster>(
    cluster,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;
  check_node_ops_allowed(&cluster)?;

  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;

  match periphery_client(&server)
    .await?
    .request(SetClusterNodeSchedulable {
      target: cluster_target(&cluster).await?,
      node,
      schedulable,
    })
    .await
  {
    Ok(log) => update.logs.push(log),
    Err(e) => update.push_error_log(
      if schedulable {
        "Uncordon Node"
      } else {
        "Cordon Node"
      },
      format_serror(&e.into()),
    ),
  }

  update.finalize();
  update_update(update.clone()).await?;
  Ok(update)
}

impl Resolve<ExecuteArgs> for DrainClusterNode {
  #[instrument(
    "DrainClusterNode",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      node = self.node,
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
    check_node_ops_allowed(&cluster)?;

    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;

    match periphery_client(&server)
      .await?
      .request(PeripheryDrainClusterNode {
        target: cluster_target(&cluster).await?,
        node: self.node,
        force: self.force,
        delete_emptydir_data: self.delete_emptydir_data,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => {
        update.push_error_log("Drain Node", format_serror(&e.into()))
      }
    }

    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for ApplyClusterObject {
  #[instrument(
    "ApplyClusterObject",
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
    let mut update = update.clone();
    if self.contents.trim().is_empty() {
      return Err(anyhow!("The manifest is empty").into());
    }
    // Applying arbitrary manifests is closer to editing the Cluster
    // than executing it, so it takes Write rather than Execute.
    let (cluster, namespace, server) = cluster_execution_setup(
      &self.cluster,
      self.namespace,
      user,
      PermissionLevel::Write,
    )
    .await?;

    if !cluster.config.cluster_resources
      && let Some(kind) = cluster_scoped_kind(&self.contents)
    {
      return Err(
        anyhow!(
          "Manifest declares cluster-scoped kind '{kind}', but this Cluster has cluster resources disabled"
        )
        .into(),
      );
    }
    // A manifest's explicit metadata.namespace overrides kubectl's
    // --namespace, so declared namespaces are checked too.
    if let Some(ns) =
      disallowed_manifest_namespace(&self.contents, &cluster.config)
    {
      return Err(
        anyhow!(
          "Manifest declares namespace '{ns}', which is not in this Cluster's allowed namespaces {:?}",
          cluster.config.namespaces
        )
        .into(),
      );
    }

    match periphery_client(&server)
      .await?
      .request(PeripheryApplyClusterObject {
        target: cluster_target(&cluster).await?,
        contents: self.contents,
        namespace,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update
        .push_error_log("Apply Object", format_serror(&e.into())),
    }

    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

/// The first `namespace:` declared in `manifests` that is outside the
/// allow-list, if any. The same deliberately shallow line scan as
/// [cluster_scoped_kind]: it errs toward refusing, which is the safe
/// direction for a blast-radius control. No-op when the Cluster does
/// not restrict namespaces.
fn disallowed_manifest_namespace(
  manifests: &str,
  config: &komodo_client::entities::cluster::ClusterConfig,
) -> Option<String> {
  if config.namespaces.is_empty() {
    return None;
  }
  manifests.lines().find_map(|line| {
    let namespace = line
      .trim()
      .strip_prefix("namespace:")?
      .trim()
      .trim_matches('"');
    (!namespace.is_empty() && !config.namespace_allowed(namespace))
      .then(|| namespace.to_string())
  })
}

/// Shared setup for single-object executions: permission check,
/// namespace resolution against the allow-list, and the Server whose
/// Periphery runs kubectl.
async fn cluster_execution_setup(
  cluster: &str,
  namespace_override: Option<String>,
  user: &User,
  level: PermissionLevel,
) -> anyhow::Result<(Cluster, String, Server)> {
  let cluster =
    get_check_permissions::<Cluster>(cluster, user, level.into())
      .await?;

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

  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;

  Ok((cluster, namespace, server))
}

/// kubectl aliases for the kinds `rollout restart` / `undo` accept.
const ROLLOUT_KINDS: &[&str] = &[
  "deployment",
  "deployments",
  "deploy",
  "statefulset",
  "statefulsets",
  "sts",
  "daemonset",
  "daemonsets",
  "ds",
];

/// kubectl aliases for the kinds `scale` accepts.
const SCALE_KINDS: &[&str] = &[
  "deployment",
  "deployments",
  "deploy",
  "statefulset",
  "statefulsets",
  "sts",
  "replicaset",
  "replicasets",
  "rs",
];

fn check_workload_kind(
  kind: &str,
  allowed: &[&str],
  verb: &str,
) -> anyhow::Result<()> {
  if allowed.contains(&kind.trim().to_lowercase().as_str()) {
    Ok(())
  } else {
    Err(anyhow!("Kind '{kind}' does not support {verb}"))
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
      wait_ready: cluster.config.wait_ready,
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

  #[test]
  fn workload_kind_gates() {
    for kind in ["deployments", "Deploy", "sts", "daemonset"] {
      assert!(
        check_workload_kind(kind, ROLLOUT_KINDS, "rollout").is_ok()
      );
    }
    for kind in ["pods", "replicasets", "nodes", ""] {
      assert!(
        check_workload_kind(kind, ROLLOUT_KINDS, "rollout").is_err()
      );
    }
    // replicasets scale but don't rollout.
    assert!(check_workload_kind("rs", SCALE_KINDS, "scale").is_ok());
    assert!(
      check_workload_kind("pods", SCALE_KINDS, "scale").is_err()
    );
  }

  #[test]
  fn scans_manifest_namespaces() {
    use komodo_client::entities::cluster::ClusterConfig;
    let restricted = ClusterConfig {
      namespaces: vec!["allowed".to_string()],
      ..Default::default()
    };
    let unrestricted = ClusterConfig::default();

    let manifest =
      "metadata:\n  name: foo\n  namespace: other\nkind: ConfigMap\n";
    assert_eq!(
      disallowed_manifest_namespace(manifest, &restricted),
      Some("other".to_string())
    );
    // Unrestricted clusters skip the scan entirely.
    assert_eq!(
      disallowed_manifest_namespace(manifest, &unrestricted),
      None
    );

    let manifest =
      "metadata:\n  name: foo\n  namespace: \"allowed\"\n";
    assert_eq!(
      disallowed_manifest_namespace(manifest, &restricted),
      None
    );
  }
}
