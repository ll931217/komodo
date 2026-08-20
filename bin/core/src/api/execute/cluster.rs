use anyhow::{Context, anyhow};
use formatting::format_serror;
use komodo_client::{
  api::execute::*,
  entities::{
    cluster::{Cluster, is_cluster_scoped_kind},
    permission::PermissionLevel,
    server::Server,
    update::{Log, Update},
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ApplyClusterObject as PeripheryApplyClusterObject,
  ClusterRolloutVerb,
  CreateClusterPortForward as PeripheryCreateClusterPortForward,
  DeleteClusterPortForward as PeripheryDeleteClusterPortForward,
  DeleteClusterResource,
  DrainClusterNode as PeripheryDrainClusterNode,
  RollbackHelmRelease as PeripheryRollbackHelmRelease,
  RolloutClusterWorkload, ScaleClusterResource,
  SetClusterNodeSchedulable,
  UninstallHelmRelease as PeripheryUninstallHelmRelease,
};

use crate::{
  helpers::{
    cluster::{
      check_kind_allowed, cluster_target_and_replacers,
      forbidden_manifest_kind,
    },
    periphery_client,
    update::update_update,
  },
  permission::get_check_permissions,
  resource,
  state::action_states,
};

use super::ExecuteArgs;

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

    check_kind_allowed(&cluster.config, &self.kind)?;

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

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard =
      action_state.update(|state| state.deleting_object = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(DeleteClusterResource {
        target,
        kind: self.kind,
        namespace,
        name: self.name,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Delete Object",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
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

  check_kind_allowed(&cluster.config, &kind)?;

  let action_state = action_states()
    .cluster
    .get_or_insert_default(&cluster.id)
    .await;
  let action_guard = action_state.update(|state| match verb {
    ClusterRolloutVerb::Restart => state.restarting_workload = true,
    ClusterRolloutVerb::Undo => state.rolling_back_workload = true,
  })?;

  let (target, secret_replacers) =
    cluster_target_and_replacers(&cluster).await?;
  match periphery_client(&server)
    .await?
    .request(RolloutClusterWorkload {
      target,
      verb,
      kind,
      name,
      namespace,
    })
    .await
  {
    Ok(log) => update.logs.push(log),
    Err(e) => update.push_error_log(
      "Rollout",
      svi::replace_in_string(
        &format_serror(&e.into()),
        &secret_replacers,
      ),
    ),
  }

  drop(action_guard);
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

    check_kind_allowed(&cluster.config, &self.kind)?;

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard =
      action_state.update(|state| state.scaling_workload = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(ScaleClusterResource {
        target,
        kind: self.kind,
        name: self.name,
        replicas: self.replicas,
        namespace,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Scale",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
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
  check_kind_allowed(&cluster.config, "Node")?;
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

  let action_state = action_states()
    .cluster
    .get_or_insert_default(&cluster.id)
    .await;
  let action_guard = action_state.update(|state| {
    if schedulable {
      state.uncordoning_node = true;
    } else {
      state.cordoning_node = true;
    }
  })?;

  let (target, secret_replacers) =
    cluster_target_and_replacers(&cluster).await?;
  match periphery_client(&server)
    .await?
    .request(SetClusterNodeSchedulable {
      target,
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
      svi::replace_in_string(
        &format_serror(&e.into()),
        &secret_replacers,
      ),
    ),
  }

  drop(action_guard);
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

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard =
      action_state.update(|state| state.draining_node = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(PeripheryDrainClusterNode {
        target,
        node: self.node,
        force: self.force,
        delete_emptydir_data: self.delete_emptydir_data,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Drain Node",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
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

    if let Some(kind) =
      forbidden_manifest_kind(&self.contents, &cluster.config)
    {
      return Err(
        anyhow!(
          "Manifest declares kind '{kind}', which this Cluster's kind policy forbids"
        )
        .into(),
      );
    }

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

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard =
      action_state.update(|state| state.applying_object = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(PeripheryApplyClusterObject {
        target,
        contents: self.contents,
        namespace,
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Apply Object",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
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
pub(super) fn disallowed_manifest_namespace(
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

/// The first cluster-scoped kind declared in `manifests`, if any.
///
/// A deliberately shallow scan of `kind:` lines rather than a full
/// parse: it errs toward refusing, which is the safe direction for a
/// blast-radius control.
pub(super) fn cluster_scoped_kind(manifests: &str) -> Option<String> {
  manifests.lines().find_map(|line| {
    let kind =
      line.trim().strip_prefix("kind:")?.trim().trim_matches('"');
    is_cluster_scoped_kind(kind).then(|| kind.to_string())
  })
}

/// Shared permission + namespace scoping for the helm executions.
async fn helm_scope(
  cluster: &str,
  namespace: Option<String>,
  user: &User,
) -> anyhow::Result<(Cluster, Server, String)> {
  let cluster = get_check_permissions::<Cluster>(
    cluster,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;
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
  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;
  Ok((cluster, server, namespace))
}

impl Resolve<ExecuteArgs> for RollbackHelmRelease {
  #[instrument(
    "RollbackHelmRelease",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      name = self.name,
      revision = self.revision,
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
    let (cluster, server, namespace) =
      helm_scope(&self.cluster, self.namespace, user).await?;

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard = action_state
      .update(|state| state.rolling_back_helm_release = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(PeripheryRollbackHelmRelease {
        target,
        name: self.name,
        namespace,
        revision: self.revision,
        secret_replacers: secret_replacers.clone(),
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Rollback Release",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for UninstallHelmRelease {
  #[instrument(
    "UninstallHelmRelease",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
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
    let (cluster, server, namespace) =
      helm_scope(&self.cluster, self.namespace, user).await?;

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard = action_state
      .update(|state| state.uninstalling_helm_release = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(PeripheryUninstallHelmRelease {
        target,
        name: self.name,
        namespace,
        secret_replacers: secret_replacers.clone(),
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Uninstall Release",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for CreateClusterPortForward {
  #[instrument(
    "CreateClusterPortForward",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
      name = self.name,
      resource = self.resource,
      local_port = self.local_port,
      remote_port = self.remote_port,
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
    let (cluster, server, namespace) =
      helm_scope(&self.cluster, self.namespace, user).await?;

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard = action_state
      .update(|state| state.creating_port_forward = true)?;

    let (target, secret_replacers) =
      cluster_target_and_replacers(&cluster).await?;
    match periphery_client(&server)
      .await?
      .request(PeripheryCreateClusterPortForward {
        target,
        // Scope the session per Cluster, so Clusters sharing a
        // Server cannot collide or see each other's sessions.
        session: format!("{}:{}", cluster.id, self.name),
        resource: self.resource,
        namespace,
        local_port: self.local_port,
        remote_port: self.remote_port,
        address: self.address.unwrap_or_default(),
      })
      .await
    {
      Ok(forward) => update.logs.push(Log::simple(
        "Create Port Forward",
        format!(
          "Forwarding {}:{} -> {}:{} on the Server",
          forward.address,
          forward.local_port,
          forward.resource,
          forward.remote_port,
        ),
      )),
      Err(e) => update.push_error_log(
        "Create Port Forward",
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      ),
    }

    drop(action_guard);
    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}

impl Resolve<ExecuteArgs> for DeleteClusterPortForward {
  #[instrument(
    "DeleteClusterPortForward",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      cluster = self.cluster,
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
    let server = resource::get::<Server>(&cluster.config.server_id)
      .await
      .context("Failed to get the Cluster's Server")?;

    let action_state = action_states()
      .cluster
      .get_or_insert_default(&cluster.id)
      .await;
    let action_guard = action_state
      .update(|state| state.deleting_port_forward = true)?;

    match periphery_client(&server)
      .await?
      .request(PeripheryDeleteClusterPortForward {
        session: format!("{}:{}", cluster.id, self.name),
      })
      .await
    {
      Ok(log) => update.logs.push(log),
      Err(e) => update.push_error_log(
        "Delete Port Forward",
        format_serror(&e.into()),
      ),
    }

    drop(action_guard);
    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
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
