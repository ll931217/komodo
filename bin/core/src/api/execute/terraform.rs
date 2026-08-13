use anyhow::Context;
use database::mungos::{by_id::update_one_by_id, mongodb::bson::doc};
use formatting::format_serror;
use komodo_client::{
  api::execute::*,
  entities::{
    permission::PermissionLevel,
    server::Server,
    terraform::{Terraform, TerraformState},
    update::Update,
    user::User,
  },
};
use mogh_resolver::Resolve;
use periphery_client::api::terraform::{RunTerraform, TerraformMode};

use crate::{
  helpers::{
    periphery_client,
    terraform::{
      InterpolatedTerraform, interpolated_terraform, terraform_source,
    },
    update::update_update,
  },
  permission::get_check_permissions,
  resource,
  state::{action_states, db_client},
};

use super::{BatchExecutionResponse, ExecuteArgs, ExecuteRequest};

impl super::BatchExecute for BatchPlanTerraform {
  type Resource = Terraform;
  fn single_request(terraform: String) -> ExecuteRequest {
    ExecuteRequest::PlanTerraform(PlanTerraform { terraform })
  }
}

impl Resolve<ExecuteArgs> for BatchPlanTerraform {
  #[instrument(
    "BatchPlanTerraform",
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
      super::batch_execute::<BatchPlanTerraform>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl super::BatchExecute for BatchApplyTerraform {
  type Resource = Terraform;
  fn single_request(terraform: String) -> ExecuteRequest {
    ExecuteRequest::ApplyTerraform(ApplyTerraform { terraform })
  }
}

impl Resolve<ExecuteArgs> for BatchApplyTerraform {
  #[instrument(
    "BatchApplyTerraform",
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
      super::batch_execute::<BatchApplyTerraform>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl super::BatchExecute for BatchDestroyTerraform {
  type Resource = Terraform;
  fn single_request(terraform: String) -> ExecuteRequest {
    ExecuteRequest::DestroyTerraform(DestroyTerraform { terraform })
  }
}

impl Resolve<ExecuteArgs> for BatchDestroyTerraform {
  #[instrument(
    "BatchDestroyTerraform",
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
      super::batch_execute::<BatchDestroyTerraform>(
        &self.pattern,
        self.tags,
        user,
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for PlanTerraform {
  #[instrument(
    "PlanTerraform",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      terraform = self.terraform,
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
      run_terraform(
        &self.terraform,
        TerraformMode::Plan,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for ApplyTerraform {
  #[instrument(
    "ApplyTerraform",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      terraform = self.terraform,
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
      run_terraform(
        &self.terraform,
        TerraformMode::Apply,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

impl Resolve<ExecuteArgs> for DestroyTerraform {
  #[instrument(
    "DestroyTerraform",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      terraform = self.terraform,
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
      run_terraform(
        &self.terraform,
        TerraformMode::Destroy,
        user,
        update.clone(),
      )
      .await?,
    )
  }
}

/// Destroy tears down everything in the unit's state, an unpinned
/// blast radius, so it asks for more than Execute - the same reasoning
/// as `ApplyClusterObject`.
fn permission_level(mode: TerraformMode) -> PermissionLevel {
  match mode {
    TerraformMode::Plan | TerraformMode::Apply => {
      PermissionLevel::Execute
    }
    TerraformMode::Destroy => PermissionLevel::Write,
  }
}

fn stage(mode: TerraformMode) -> &'static str {
  match mode {
    TerraformMode::Plan => "Plan",
    TerraformMode::Apply => "Apply",
    TerraformMode::Destroy => "Destroy",
  }
}

/// Shared plan / apply / destroy path.
async fn run_terraform(
  terraform: &str,
  mode: TerraformMode,
  user: &User,
  mut update: Update,
) -> anyhow::Result<Update> {
  let terraform = get_check_permissions::<Terraform>(
    terraform,
    user,
    permission_level(mode).into(),
  )
  .await?;

  // Held for the whole execution: every mode shares one working
  // directory and one state file, so two at once corrupt each other.
  // Terraform's own state lock is the second layer, not the first.
  let action_state = action_states()
    .terraform
    .get_or_insert_default(&terraform.id)
    .await;
  let action_guard = action_state.update(|state| match mode {
    TerraformMode::Plan => state.planning = true,
    TerraformMode::Apply => state.applying = true,
    TerraformMode::Destroy => state.destroying = true,
  })?;

  let server = resource::get::<Server>(&terraform.config.server_id)
    .await
    .context("Failed to get the Terraform's Server")?;

  let InterpolatedTerraform {
    file_contents,
    environment,
    secret_replacers,
  } = interpolated_terraform(&terraform).await?;
  let source = terraform_source(&terraform, file_contents).await?;

  let res = periphery_client(&server)
    .await?
    .request(RunTerraform {
      name: terraform.name.clone(),
      source,
      run_directory: terraform.config.run_directory.clone(),
      mode,
      managed_state: terraform.config.managed_state,
      environment,
      // The Cluster kubeconfig bridge is wired in a later phase.
      kubeconfig_contents: String::new(),
      kubeconfig_path: String::new(),
      proxy_url: terraform.config.proxy_url.clone(),
      no_proxy: terraform.config.no_proxy.clone(),
      extra_args: terraform.config.extra_args.clone(),
      secret_replacers: secret_replacers.clone(),
    })
    .await;

  // Free the resource before the Update goes out: that broadcast is
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
      set_state(&terraform.id, TerraformState::Failed).await;
      update_update(update.clone()).await?;
      return Ok(update);
    }
  };

  let changes = res.changes;
  update.logs.extend(res.logs);
  // Record what was run, for repo sources.
  if let Some(hash) = res.commit_hash {
    update.commit_hash = hash;
  }
  update.finalize();

  set_state(&terraform.id, run_state(update.success, mode, changes))
    .await;

  update_update(update.clone()).await?;

  Ok(update)
}

/// What the resource's state becomes after a run.
///
/// A plan that finds changes still succeeds (`-detailed-exitcode`
/// returns 2), which is exactly the case a bare `success` flag cannot
/// express - hence Drifted.
fn run_state(
  success: bool,
  mode: TerraformMode,
  changes: Option<bool>,
) -> TerraformState {
  if !success {
    return TerraformState::Failed;
  }
  match mode {
    TerraformMode::Plan => match changes {
      Some(true) => TerraformState::Drifted,
      Some(false) => TerraformState::Ok,
      // Plan succeeded without reporting an exit code: saying "Ok"
      // here would claim there is no drift, which was not measured.
      None => TerraformState::Unknown,
    },
    // Apply and destroy leave the infrastructure matching the state
    // they just wrote.
    TerraformMode::Apply | TerraformMode::Destroy => {
      TerraformState::Ok
    }
  }
}

/// Persisted on the resource rather than derived from the Update: only
/// the run knows whether a successful plan found drift.
async fn set_state(id: &str, state: TerraformState) {
  if let Err(e) = update_one_by_id(
    &db_client().terraforms,
    id,
    database::mungos::update::Update::Set(
      doc! { "info.state": state.to_string() },
    ),
    None,
  )
  .await
  {
    warn!("Failed to update Terraform state | {id} | {e:#}");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plan_with_changes_is_drift_not_failure() {
    assert_eq!(
      run_state(true, TerraformMode::Plan, Some(true)),
      TerraformState::Drifted
    );
    assert_eq!(
      run_state(true, TerraformMode::Plan, Some(false)),
      TerraformState::Ok
    );
    // Never claim "no drift" from a run that did not report one.
    assert_eq!(
      run_state(true, TerraformMode::Plan, None),
      TerraformState::Unknown
    );
  }

  #[test]
  fn failure_beats_every_mode() {
    for mode in [
      TerraformMode::Plan,
      TerraformMode::Apply,
      TerraformMode::Destroy,
    ] {
      assert_eq!(
        run_state(false, mode, Some(true)),
        TerraformState::Failed
      );
    }
  }

  /// Destroy is the one verb that needs more than Execute.
  #[test]
  fn destroy_needs_write() {
    assert_eq!(
      permission_level(TerraformMode::Plan),
      PermissionLevel::Execute
    );
    assert_eq!(
      permission_level(TerraformMode::Apply),
      PermissionLevel::Execute
    );
    assert_eq!(
      permission_level(TerraformMode::Destroy),
      PermissionLevel::Write
    );
  }
}
