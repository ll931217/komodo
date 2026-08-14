use anyhow::{Context, anyhow};
use database::mungos::{by_id::update_one_by_id, mongodb::bson::doc};
use formatting::format_serror;
use komodo_client::{
  api::execute::*,
  entities::{
    application::{
      Application, ApplicationSourceKind, ApplicationState,
    },
    permission::PermissionLevel,
    server::Server,
    update::Update,
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ApplyClusterManifests, ClusterApplyMode,
};

use crate::{
  helpers::{
    application::{
      InterpolatedApplication, application_cluster,
      application_manifest_source, interpolated_application,
    },
    cluster::cluster_target_and_replacers,
    periphery_client,
    update::update_update,
  },
  monitor::alert::application::alert_application_state,
  permission::get_check_permissions,
  resource,
  state::{action_states, db_client},
};

use super::{
  BatchExecutionResponse, ExecuteArgs, ExecuteRequest,
  cluster::{cluster_scoped_kind, disallowed_manifest_namespace},
};

impl super::BatchExecute for BatchDeployApplication {
  type Resource = Application;
  fn single_request(application: String) -> ExecuteRequest {
    ExecuteRequest::DeployApplication(DeployApplication {
      application,
      namespace: None,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDeployApplication {
  #[instrument(
    "BatchDeployApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
      tags = self.tags.join(","),
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchDeployApplication>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl super::BatchExecute for BatchDestroyApplication {
  type Resource = Application;
  fn single_request(application: String) -> ExecuteRequest {
    ExecuteRequest::DestroyApplication(DestroyApplication {
      application,
      namespace: None,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDestroyApplication {
  #[instrument(
    "BatchDestroyApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
      tags = self.tags.join(","),
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchDestroyApplication>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl super::BatchExecute for BatchDiffApplication {
  type Resource = Application;
  fn single_request(application: String) -> ExecuteRequest {
    ExecuteRequest::DiffApplication(DiffApplication {
      application,
      namespace: None,
    })
  }
}

impl Resolve<ExecuteArgs> for BatchDiffApplication {
  #[instrument(
    "BatchDiffApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      pattern = self.pattern,
      tags = self.tags.join(","),
    )
  )]
  async fn resolve(
    self,
    ExecuteArgs { user, task_id, .. }: &ExecuteArgs,
  ) -> mogh_error::Result<BatchExecutionResponse> {
    Ok(
      super::batch_execute::<BatchDiffApplication>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DeployApplication {
  #[instrument(
    "DeployApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      application = self.application,
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
        &self.application,
        self.namespace,
        ClusterApplyMode::Apply,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DestroyApplication {
  #[instrument(
    "DestroyApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      application = self.application,
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
        &self.application,
        self.namespace,
        ClusterApplyMode::Delete,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DiffApplication {
  #[instrument(
    "DiffApplication",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      application = self.application,
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
        &self.application,
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

/// Shared deploy / destroy / diff path.
///
/// Every scoping control comes from the **Cluster**, not from the
/// Application: an Application may only target a namespace its Cluster
/// permits, and may only declare cluster-scoped objects if its Cluster
/// allows them at all. That is the whole point of the split - the
/// policy lives with the credentials, and an Application cannot widen
/// it by editing itself.
async fn execute_manifests(
  application: &str,
  namespace_override: Option<String>,
  mode: ClusterApplyMode,
  user: &User,
  mut update: Update,
) -> anyhow::Result<Update> {
  let application = get_check_permissions::<Application>(
    application,
    user,
    PermissionLevel::Execute.into(),
  )
  .await?;

  // Held for the whole execution: apply / delete / diff share a
  // manifest clone directory, so two at once corrupt each other's
  // checkout even when they target different namespaces.
  let action_state = action_states()
    .application
    .get_or_insert_default(&application.id)
    .await;
  let action_guard = action_state.update(|state| match mode {
    ClusterApplyMode::Apply => state.deploying = true,
    ClusterApplyMode::Delete => state.destroying = true,
    ClusterApplyMode::Diff => state.diffing = true,
  })?;

  // Only the Contents source needs file_contents; the others read
  // from the host or a repo, where an empty field is expected.
  if application.config.manifest_source()
    == ApplicationSourceKind::Contents
    && application.config.file_contents.trim().is_empty()
  {
    return Err(anyhow!("Application has no manifests configured"));
  }

  let cluster = application_cluster(&application).await?;

  let namespace = match namespace_override {
    Some(namespace) if !namespace.is_empty() => namespace,
    _ if !application.config.namespace.is_empty() => {
      application.config.namespace.clone()
    }
    _ => cluster.config.default_namespace().to_string(),
  };
  if !cluster.config.namespace_allowed(&namespace) {
    return Err(anyhow!(
      "Namespace '{namespace}' is not in Cluster {}'s allowed namespaces {:?}",
      cluster.name,
      cluster.config.namespaces
    ));
  }

  // A kustomization can set `namespace:` itself, and those objects
  // land wherever it says rather than where the request said. Scan for
  // it, so the allow-list is not silently bypassed by the manifests.
  if let Some(declared) = disallowed_manifest_namespace(
    &application.config.file_contents,
    &cluster.config,
  ) {
    return Err(anyhow!(
      "Manifests declare namespace '{declared}', which is not in Cluster {}'s allowed namespaces {:?}",
      cluster.name,
      cluster.config.namespaces
    ));
  }

  if !cluster.config.cluster_resources
    && let Some(kind) =
      cluster_scoped_kind(&application.config.file_contents)
  {
    return Err(anyhow!(
      "Manifests declare cluster-scoped kind '{kind}', but Cluster {} has cluster resources disabled",
      cluster.name
    ));
  }

  let server = resource::get::<Server>(&cluster.config.server_id)
    .await
    .context("Failed to get the Cluster's Server")?;

  // Two interpolation passes on purpose: the kubeconfig belongs to the
  // Cluster and the manifests to the Application, and each carries its
  // own skip_secret_interp. Both sets of replacers scrub the output.
  let (target, mut secret_replacers) =
    cluster_target_and_replacers(&cluster).await?;
  let InterpolatedApplication {
    manifests,
    secret_replacers: mut manifest_replacers,
  } = interpolated_application(&application).await?;
  secret_replacers.append(&mut manifest_replacers);

  let source =
    application_manifest_source(&application, manifests).await?;

  let res = periphery_client(&server)
    .await?
    .request(ApplyClusterManifests {
      target,
      source,
      namespace,
      kustomize: application.config.kustomize,
      mode,
      extra_args: application.config.extra_args.clone(),
      secret_replacers: secret_replacers.clone(),
      wait_ready: application.config.wait_ready,
    })
    .await;

  // Free the Application before the Update goes out: that broadcast is
  // what makes clients refetch the action state.
  drop(action_guard);

  let res = match res {
    Ok(res) => res,
    Err(e) => {
      // Periphery sanitizes what it logs itself, but an error raised
      // before it ever answers is formatted here, and the request it
      // carries is built from interpolated config.
      update.push_error_log(
        stage(mode),
        svi::replace_in_string(
          &format_serror(&e.into()),
          &secret_replacers,
        ),
      );
      update.finalize();
      record_state(&application, run_state(false, mode, None)).await;
      update_update(update.clone()).await?;
      return Ok(update);
    }
  };

  let changes = res.changes;
  update.logs.extend(res.logs);
  // Record what was deployed, for repo sources.
  if let Some(hash) = res.commit_hash {
    update.commit_hash = hash;
  }
  update.finalize();

  record_state(
    &application,
    run_state(update.success, mode, changes),
  )
  .await;

  update_update(update.clone()).await?;

  Ok(update)
}

/// What the resource's state becomes after an execution.
///
/// A Diff that finds differences still succeeded - `kubectl diff`
/// exits 1 to say "differences", which Periphery maps to success and
/// reports through `changes`. That is Drifted, not Failed. A Diff that
/// reports no verdict at all leaves the state alone rather than
/// claiming health nobody measured.
fn run_state(
  success: bool,
  mode: ClusterApplyMode,
  changes: Option<bool>,
) -> Option<ApplicationState> {
  if !success {
    return Some(ApplicationState::Failed);
  }
  match mode {
    ClusterApplyMode::Apply => Some(ApplicationState::Deployed),
    // Nothing is deployed any more, and Deployed would be a lie.
    ClusterApplyMode::Delete => Some(ApplicationState::Unknown),
    ClusterApplyMode::Diff => match changes {
      Some(true) => Some(ApplicationState::Drifted),
      Some(false) => Some(ApplicationState::Deployed),
      None => None,
    },
  }
}

/// Persist the execution's verdict, then alert on it.
///
/// Both halves belong to the execution: nothing polls an Application,
/// so this is the only moment either can be known.
async fn record_state(
  application: &Application,
  state: Option<ApplicationState>,
) {
  let Some(state) = state else {
    return;
  };
  set_state(&application.id, Some(state)).await;
  alert_application_state(application, state).await;
}

async fn set_state(id: &str, state: Option<ApplicationState>) {
  let Some(state) = state else {
    return;
  };
  if let Err(e) = update_one_by_id(
    &db_client().applications,
    id,
    database::mungos::update::Update::Set(
      doc! { "info.state": state.to_string() },
    ),
    None,
  )
  .await
  {
    warn!("Failed to update Application state | {id} | {e:#}");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn diff_with_differences_is_drift_not_failure() {
    assert_eq!(
      run_state(true, ClusterApplyMode::Diff, Some(true)),
      Some(ApplicationState::Drifted)
    );
    assert_eq!(
      run_state(true, ClusterApplyMode::Diff, Some(false)),
      Some(ApplicationState::Deployed)
    );
  }

  #[test]
  fn diff_without_a_verdict_claims_nothing() {
    // An older Periphery does not send `changes`. Writing Deployed
    // there would assert health that was never measured.
    assert_eq!(run_state(true, ClusterApplyMode::Diff, None), None);
  }

  #[test]
  fn failure_beats_the_verb() {
    for mode in [
      ClusterApplyMode::Apply,
      ClusterApplyMode::Delete,
      ClusterApplyMode::Diff,
    ] {
      assert_eq!(
        run_state(false, mode, Some(false)),
        Some(ApplicationState::Failed)
      );
    }
  }

  #[test]
  fn destroy_does_not_leave_it_deployed() {
    assert_eq!(
      run_state(true, ClusterApplyMode::Apply, None),
      Some(ApplicationState::Deployed)
    );
    assert_eq!(
      run_state(true, ClusterApplyMode::Delete, None),
      Some(ApplicationState::Unknown)
    );
  }
}
