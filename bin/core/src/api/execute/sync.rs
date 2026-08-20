use std::{collections::HashMap, str::FromStr};

use anyhow::anyhow;
use database::mungos::{
  by_id::update_one_by_id,
  mongodb::bson::{doc, oid::ObjectId},
};
use formatting::{Color, colored, format_serror};
use komodo_client::{
  api::{
    execute::{CancelSync, RunSync},
    write::RefreshResourceSyncPending,
  },
  entities::{
    self, ResourceTargetVariant,
    action::Action,
    alerter::Alerter,
    application::Application,
    build::Build,
    builder::Builder,
    cluster::Cluster,
    deployment::Deployment,
    komodo_timestamp,
    permission::PermissionLevel,
    procedure::Procedure,
    repo::Repo,
    server::Server,
    stack::Stack,
    swarm::Swarm,
    sync::ResourceSync,
    terraform::Terraform,
    update::{Log, Update},
    user::sync_user,
  },
};
use mogh_resolver::Resolve;

use tokio_util::sync::CancellationToken;

use crate::{
  api::write::WriteArgs,
  helpers::{
    all_resources::AllResourcesById, query::get_id_to_tags,
    retry::maybe_retry, update::update_update,
    window::check_execution_window,
  },
  permission::get_check_permissions,
  state::{action_states, db_client, sync_cancel_cache},
  sync::{
    ResourceSyncTrait,
    deploy::{
      SyncDeployParams, build_deploy_cache, deploy_from_cache,
    },
    execute::{ExecuteResourceSync, get_updates_for_execution},
    remote::RemoteResources,
  },
};

use super::{ExecuteArgs, ExecuteRequest};

impl Resolve<ExecuteArgs> for RunSync {
  #[instrument(
    "RunSync",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      sync = self.sync,
      resource_type = format!("{:?}", self.resource_type),
      resources = format!("{:?}", self.resources),
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
    // Cloned before `self` is destructured: a retry re-runs the
    // same request through /execute, which is what gives each
    // attempt its own Update.
    let retry_request = ExecuteRequest::RunSync(self.clone());
    let RunSync {
      sync,
      resource_type: match_resource_type,
      resources: match_resources,
      dry_run,
      confirm_deletes,
    } = self;
    let sync = get_check_permissions::<entities::sync::ResourceSync>(
      &sync,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    let repo = if !sync.config.files_on_host
      && !sync.config.linked_repo.is_empty()
    {
      crate::resource::get::<Repo>(&sync.config.linked_repo)
        .await?
        .into()
    } else {
      None
    };

    // A dry run changes nothing, so a closed window has no reason
    // to refuse it - it is how you find out what a real run would do.
    let window_override = if dry_run {
      None
    } else {
      check_execution_window(&sync.config.execution_windows, user)?
    };

    // get the action state for the sync (or insert default).
    let action_state =
      action_states().sync.get_or_insert_default(&sync.id).await;

    // This will set action state back to default when dropped.
    // Will also check to ensure sync not already busy before updating.
    let action_guard =
      action_state.update(|state| state.syncing = true)?;

    let mut update = update.clone();

    if let Some(note) = window_override {
      update.push_simple_log("Execution Window", note);
    }

    // Send update here for FE to recheck action state
    update_update(update.clone()).await?;

    let remote = match crate::sync::remote::get_remote_resources(
      &sync,
      repo.as_ref(),
    )
    .await
    {
      Ok(remote) => remote,
      Err(e) => {
        // Recorded on the Update rather than returned as an Err: a
        // failed clone is the most common transient sync failure,
        // and the retry policy only sees failures that reached an
        // Update.
        update.push_error_log(
          "Get Remote Resources",
          format_serror(
            &e.context("failed to get remote resources").into(),
          ),
        );
        update.finalize();
        maybe_retry(
          &mut update,
          &sync.config.retry,
          retry_request,
          user,
        )
        .await;
        drop(action_guard);
        update_update(update.clone()).await?;
        return Ok(update);
      }
    };
    let RemoteResources {
      resources,
      logs,
      hash,
      message,
      file_errors,
      ..
    } = remote;

    update.logs.extend(logs);
    update_update(update.clone()).await?;

    if !file_errors.is_empty() {
      return Err(
        anyhow!("Found file errors. Cannot execute sync.").into(),
      );
    }

    let resources = resources?;

    let id_to_tags = get_id_to_tags(None).await?;
    let all_resources = AllResourcesById::load().await?;
    // Convert all match_resources to names
    let match_resources = match_resources.map(|resources| {
      resources
        .into_iter()
        .filter_map(|name_or_id| {
          let Some(resource_type) = match_resource_type else {
            return Some(name_or_id);
          };
          macro_rules! resolve_id_to_name {
            ($(($Variant:ident, $field:ident)),* $(,)?) => {
              match ObjectId::from_str(&name_or_id) {
                Ok(_) => match resource_type {
                  $(
                    ResourceTargetVariant::$Variant => all_resources
                      .$field
                      .get(&name_or_id)
                      .map(|r| r.name.clone()),
                  )*
                  ResourceTargetVariant::System => None,
                },
                Err(_) => Some(name_or_id),
              }
            };
          }
          // New resource types need to be added here manually.
          resolve_id_to_name!(
            (Server, servers),
            (Swarm, swarms),
            (Cluster, clusters),
            (Terraform, terraforms),
            (Application, applications),
            (Stack, stacks),
            (Deployment, deployments),
            (Build, builds),
            (Repo, repos),
            (Procedure, procedures),
            (Action, actions),
            (ResourceSync, syncs),
            (Builder, builders),
            (Alerter, alerters),
          )
        })
        .collect::<Vec<_>>()
    });

    let deployments_by_name = all_resources
      .deployments
      .values()
      .filter(|deployment| {
        Deployment::include_resource(
          &deployment.name,
          &deployment.config,
          match_resource_type,
          match_resources.as_deref(),
          &deployment.tags,
          &id_to_tags,
          &sync.config.match_tags,
        )
      })
      .map(|deployment| (deployment.name.clone(), deployment.clone()))
      .collect::<HashMap<_, _>>();
    let stacks_by_name = all_resources
      .stacks
      .values()
      .filter(|stack| {
        Stack::include_resource(
          &stack.name,
          &stack.config,
          match_resource_type,
          match_resources.as_deref(),
          &stack.tags,
          &id_to_tags,
          &sync.config.match_tags,
        )
      })
      .map(|stack| (stack.name.clone(), stack.clone()))
      .collect::<HashMap<_, _>>();

    let deploy_cache = build_deploy_cache(SyncDeployParams {
      deployments: &resources.deployments,
      deployment_map: &deployments_by_name,
      stacks: &resources.stacks,
      stack_map: &stacks_by_name,
    })
    .await?;

    let delete = sync.config.managed || sync.config.delete;

    macro_rules! get_deltas {
      ($(($var:ident, $Type:ident, $field:ident)),* $(,)?) => {
        $(
          let mut $var = if sync.config.include_resources {
            get_updates_for_execution::<$Type>(
              resources.$field,
              delete,
              match_resource_type,
              match_resources.as_deref(),
              &id_to_tags,
              &sync.config.match_tags,
              &sync.config.retain_tags,
            )
            .await?
          } else {
            Default::default()
          };
        )*
      };
    }
    // New resource types need to be added here manually.
    get_deltas!(
      (server_deltas, Server, servers),
      (swarm_deltas, Swarm, swarms),
      (cluster_deltas, Cluster, clusters),
      (terraform_deltas, Terraform, terraforms),
      (application_deltas, Application, applications),
      (stack_deltas, Stack, stacks),
      (deployment_deltas, Deployment, deployments),
      (build_deltas, Build, builds),
      (repo_deltas, Repo, repos),
      (procedure_deltas, Procedure, procedures),
      (action_deltas, Action, actions),
      (builder_deltas, Builder, builders),
      (alerter_deltas, Alerter, alerters),
      (resource_sync_deltas, ResourceSync, resource_syncs),
    );

    let (
      variables_to_create,
      variables_to_update,
      mut variables_to_delete,
    ) = if match_resource_type.is_none()
      && match_resources.is_none()
      && sync.config.include_variables
    {
      // Variable writes are admin-only everywhere else (every handler in
      // api/write/variable.rs gates on user.admin), but this path applies
      // them as sync_user(), a synthetic identity hardcoded to admin:true.
      // Without this check, Execute on one ResourceSync silently confers
      // admin-equivalent power over every global Variable — including
      // flipping is_secret off, which exposes a secret to all users.
      //
      // Refused rather than silently skipped: a sync that quietly declined
      // to apply half of what it was asked to apply is the worse failure.
      if !user.admin {
        return Err(
          anyhow!(
            "This Sync has 'include_variables' enabled, and applying Variables requires admin. Either run it as an admin, or disable 'include_variables' on the Sync."
          )
          .into(),
        );
      }
      crate::sync::variables::get_updates_for_execution(
        resources.variables,
        delete,
      )
      .await?
    } else {
      Default::default()
    };
    let (
      user_groups_to_create,
      user_groups_to_update,
      mut user_groups_to_delete,
    ) = if match_resource_type.is_none()
      && match_resources.is_none()
      && sync.config.include_user_groups
    {
      crate::sync::user_groups::get_updates_for_execution(
        resources.user_groups,
        delete,
      )
      .await?
    } else {
      Default::default()
    };

    // New resource types need to be added here manually.
    if deploy_cache.is_empty()
      && resource_sync_deltas.no_changes()
      && server_deltas.no_changes()
      && swarm_deltas.no_changes()
      && cluster_deltas.no_changes()
      && terraform_deltas.no_changes()
      && application_deltas.no_changes()
      && deployment_deltas.no_changes()
      && stack_deltas.no_changes()
      && build_deltas.no_changes()
      && builder_deltas.no_changes()
      && alerter_deltas.no_changes()
      && repo_deltas.no_changes()
      && procedure_deltas.no_changes()
      && action_deltas.no_changes()
      && user_groups_to_create.is_empty()
      && user_groups_to_update.is_empty()
      && user_groups_to_delete.is_empty()
      && variables_to_create.is_empty()
      && variables_to_update.is_empty()
      && variables_to_delete.is_empty()
    {
      update.push_simple_log(
        "No Changes",
        format!(
          "{}. exiting.",
          colored("nothing to do", Color::Green)
        ),
      );
      update.finalize();

      // Drop action guard before updating
      // clients to requery action state
      drop(action_guard);
      update_update(update.clone()).await?;
      return Ok(update);
    }

    // A dry run stops here: the deltas above are exactly what the
    // batches below would apply, so reporting them and returning says
    // what this run WOULD do without doing any of it. Deliberately
    // after the no-changes check, so a dry run of an already-synced
    // ResourceSync says "nothing to do" like a real one.
    if dry_run {
      let mut sections = [
        server_deltas.dry_run_summary("Server"),
        stack_deltas.dry_run_summary("Stack"),
        deployment_deltas.dry_run_summary("Deployment"),
        build_deltas.dry_run_summary("Build"),
        builder_deltas.dry_run_summary("Builder"),
        alerter_deltas.dry_run_summary("Alerter"),
        repo_deltas.dry_run_summary("Repo"),
        procedure_deltas.dry_run_summary("Procedure"),
        action_deltas.dry_run_summary("Action"),
        resource_sync_deltas.dry_run_summary("ResourceSync"),
        cluster_deltas.dry_run_summary("Cluster"),
        application_deltas.dry_run_summary("Application"),
        terraform_deltas.dry_run_summary("Terraform"),
        swarm_deltas.dry_run_summary("Swarm"),
      ]
      .into_iter()
      .flatten()
      .collect::<Vec<_>>();

      if !variables_to_create.is_empty()
        || !variables_to_update.is_empty()
        || !variables_to_delete.is_empty()
      {
        sections.push(format!(
          "Variable:\n  create: {}\n  update: {}\n  delete: {}",
          variables_to_create.len(),
          variables_to_update.len(),
          variables_to_delete.len()
        ));
      }
      if !user_groups_to_create.is_empty()
        || !user_groups_to_update.is_empty()
        || !user_groups_to_delete.is_empty()
      {
        sections.push(format!(
          "UserGroup:\n  create: {}\n  update: {}\n  delete: {}",
          user_groups_to_create.len(),
          user_groups_to_update.len(),
          user_groups_to_delete.len()
        ));
      }

      update.push_simple_log(
        "Dry Run",
        format!(
          "{} - nothing was applied.\n\n{}",
          colored("dry run", Color::Blue),
          sections.join("\n\n")
        ),
      );
      update.finalize();
      drop(action_guard);
      update_update(update.clone()).await?;
      return Ok(update);
    }

    // Deletions come out of the per-type batches when they have to be
    // deferred: either this sync requires confirmation and this run
    // did not confirm, or `prune_last` asks for them at the end.
    //
    // Taken after the no-changes and dry-run checks on purpose: a run
    // whose only pending change is an unconfirmed deletion still has
    // something to report, and reporting "nothing to do" would be a
    // lie that hides a pending prune.
    let apply_deletes =
      !sync.config.confirm_deletes || confirm_deletes;
    let defer_deletes = !apply_deletes || sync.config.prune_last;

    macro_rules! take_deletes {
      ($(($var:ident, $deletes:ident)),* $(,)?) => {
        $(
          let $deletes = if defer_deletes {
            std::mem::take(&mut $var.to_delete)
          } else {
            Vec::new()
          };
        )*
      };
    }
    take_deletes!(
      (server_deltas, server_deletes),
      (swarm_deltas, swarm_deletes),
      (cluster_deltas, cluster_deletes),
      (terraform_deltas, terraform_deletes),
      (application_deltas, application_deletes),
      (stack_deltas, stack_deletes),
      (deployment_deltas, deployment_deletes),
      (build_deltas, build_deletes),
      (repo_deltas, repo_deletes),
      (procedure_deltas, procedure_deletes),
      (action_deltas, action_deletes),
      (builder_deltas, builder_deletes),
      (alerter_deltas, alerter_deletes),
      (resource_sync_deltas, resource_sync_deletes),
    );

    if !apply_deletes {
      let pending = [
        ("Server", &server_deletes),
        ("Swarm", &swarm_deletes),
        ("Cluster", &cluster_deletes),
        ("Terraform", &terraform_deletes),
        ("Application", &application_deletes),
        ("Stack", &stack_deletes),
        ("Deployment", &deployment_deletes),
        ("Build", &build_deletes),
        ("Repo", &repo_deletes),
        ("Procedure", &procedure_deletes),
        ("Action", &action_deletes),
        ("Builder", &builder_deletes),
        ("Alerter", &alerter_deletes),
        ("ResourceSync", &resource_sync_deletes),
      ]
      .into_iter()
      .filter(|(_, names)| !names.is_empty())
      .map(|(resource_type, names)| {
        format!("{resource_type}: {}", names.join(", "))
      })
      .collect::<Vec<_>>();

      // Variables and UserGroups are not per-type batches, so they
      // need dropping here too. A confirmation gate that held back
      // resource deletions while still deleting Variables would be
      // worse than no gate: it reads as "nothing was deleted".
      let mut pending = pending;
      if !variables_to_delete.is_empty() {
        pending.push(format!(
          "Variable: {}",
          variables_to_delete.join(", ")
        ));
        variables_to_delete.clear();
      }
      if !user_groups_to_delete.is_empty() {
        pending.push(format!(
          "UserGroup: {}",
          user_groups_to_delete
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
        ));
        user_groups_to_delete.clear();
      }

      if pending.is_empty() {
        update.push_simple_log(
          "Deletions",
          String::from("Nothing to delete."),
        );
      } else {
        update.push_simple_log(
          "Deletions Await Confirmation",
          format!(
            "{} deletions were NOT applied. Everything else in this run was.\n\n{}\n\nRun the sync again with 'confirm_deletes' to apply them.",
            colored("These", Color::Red),
            pending.join("\n")
          ),
        );
      }
    }

    // One token per ResourceSync, so CancelSync can reach a run that
    // is already in flight. Registered before the first batch and
    // cleared in every exit path below.
    let cancel = CancellationToken::new();
    sync_cancel_cache()
      .insert(sync.id.clone(), cancel.clone())
      .await;

    // =====================================================
    // The ordering these are executed does matter, since
    // latter resources may depend on prior synced resources
    // already being updated with the declared state.
    // =====================================================

    // No deps
    sync_batch(
      &mut update.logs,
      &cancel,
      crate::sync::variables::run_updates(
        variables_to_create,
        variables_to_update,
        variables_to_delete,
      ),
    )
    .await;
    sync_batch(
      &mut update.logs,
      &cancel,
      crate::sync::user_groups::run_updates(
        user_groups_to_create,
        user_groups_to_update,
        user_groups_to_delete,
      ),
    )
    .await;

    sync_batch(
      &mut update.logs,
      &cancel,
      Server::execute_sync_updates(server_deltas),
    )
    .await;
    sync_batch(
      &mut update.logs,
      &cancel,
      Alerter::execute_sync_updates(alerter_deltas),
    )
    .await;
    sync_batch(
      &mut update.logs,
      &cancel,
      Action::execute_sync_updates(action_deltas),
    )
    .await;

    // Depends on server
    sync_batch(
      &mut update.logs,
      &cancel,
      Swarm::execute_sync_updates(swarm_deltas),
    )
    .await;
    // Depends on server
    sync_batch(
      &mut update.logs,
      &cancel,
      Cluster::execute_sync_updates(cluster_deltas),
    )
    .await;
    // Depends on server, and on cluster for the kubeconfig bridge
    sync_batch(
      &mut update.logs,
      &cancel,
      Terraform::execute_sync_updates(terraform_deltas),
    )
    .await;
    // Depends on cluster, which supplies its connection and its policy
    sync_batch(
      &mut update.logs,
      &cancel,
      Application::execute_sync_updates(application_deltas),
    )
    .await;
    // Depends on server
    sync_batch(
      &mut update.logs,
      &cancel,
      Builder::execute_sync_updates(builder_deltas),
    )
    .await;
    // Depends on server / builder
    sync_batch(
      &mut update.logs,
      &cancel,
      Repo::execute_sync_updates(repo_deltas),
    )
    .await;

    // Depends on builder / repo
    sync_batch(
      &mut update.logs,
      &cancel,
      Build::execute_sync_updates(build_deltas),
    )
    .await;
    // Depends on server / repo
    sync_batch(
      &mut update.logs,
      &cancel,
      Stack::execute_sync_updates(stack_deltas),
    )
    .await;
    // Depends on repo
    sync_batch(
      &mut update.logs,
      &cancel,
      ResourceSync::execute_sync_updates(resource_sync_deltas),
    )
    .await;
    // Depends on server / build
    sync_batch(
      &mut update.logs,
      &cancel,
      Deployment::execute_sync_updates(deployment_deltas),
    )
    .await;
    // Depends on everything
    sync_batch(
      &mut update.logs,
      &cancel,
      Procedure::execute_sync_updates(procedure_deltas),
    )
    .await;

    // Execute the deploy cache
    if !cancel.is_cancelled() {
      deploy_from_cache(
        deploy_cache,
        &mut update.logs,
        sync.config.wave_delay_seconds,
      )
      .await;
    }

    // prune_last: every deletion, after every create, update and
    // deploy, in the reverse of the order those ran. A Server deleted
    // before the Deployments that referenced it is a delete that
    // fails for a reason nobody asked about.
    if apply_deletes && sync.config.prune_last {
      sync_batch(
        &mut update.logs,
        &cancel,
        ResourceSync::execute_sync_deletes(resource_sync_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Alerter::execute_sync_deletes(alerter_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Builder::execute_sync_deletes(builder_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Action::execute_sync_deletes(action_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Procedure::execute_sync_deletes(procedure_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Repo::execute_sync_deletes(repo_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Build::execute_sync_deletes(build_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Deployment::execute_sync_deletes(deployment_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Stack::execute_sync_deletes(stack_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Application::execute_sync_deletes(application_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Terraform::execute_sync_deletes(terraform_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Cluster::execute_sync_deletes(cluster_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Swarm::execute_sync_deletes(swarm_deletes),
      )
      .await;
      sync_batch(
        &mut update.logs,
        &cancel,
        Server::execute_sync_deletes(server_deletes),
      )
      .await;
    }

    sync_cancel_cache().remove(&sync.id).await;
    if cancel.is_cancelled() {
      // Say so in the audit trail. Without this the Update is just a
      // sync that did less than the diff promised, with no reason
      // recorded anywhere.
      update.push_error_log(
        komodo_client::entities::update::CANCELLED_LOG_STAGE,
        String::from(
          "Sync cancelled; resources already applied above are unchanged by the cancellation.",
        ),
      );
    }

    let db = db_client();

    if let Err(e) = update_one_by_id(
      &db.resource_syncs,
      &sync.id,
      doc! {
        "$set": {
          "info.last_sync_ts": komodo_timestamp(),
          "info.last_sync_hash": hash,
          "info.last_sync_message": message,
        }
      },
      None,
    )
    .await
    {
      warn!(
        "failed to update resource sync {} info after sync | {e:#}",
        sync.name
      )
    }

    if let Err(e) = (RefreshResourceSyncPending { sync: sync.id })
      .resolve(&WriteArgs {
        user: sync_user().to_owned(),
      })
      .await
    {
      warn!(
        "failed to refresh sync {} after run | {:#}",
        sync.name, e.error
      );
      update.push_error_log(
        "refresh sync",
        format_serror(
          &e.error
            .context("failed to refresh sync pending after run")
            .into(),
        ),
      );
    }

    update.finalize();

    maybe_retry(&mut update, &sync.config.retry, retry_request, user)
      .await;

    // Drop action guard before updating
    // clients to requery action state
    drop(action_guard);
    update_update(update.clone()).await?;

    Ok(update)
  }
}

fn maybe_extend(logs: &mut Vec<Log>, log: Option<Log>) {
  if let Some(log) = log {
    logs.push(log);
  }
}

/// Run one batch of sync updates, unless the run has been cancelled.
///
/// Takes the future rather than the result: an async fn call is lazy,
/// so a cancelled run never starts the work. Every batch goes through
/// here - a check at only some of the call sites would make
/// cancellation depend on which resource type happened to be next.
async fn sync_batch(
  logs: &mut Vec<Log>,
  cancel: &CancellationToken,
  batch: impl std::future::Future<Output = Option<Log>>,
) {
  if cancel.is_cancelled() {
    return;
  }
  maybe_extend(logs, batch.await);
}

impl Resolve<ExecuteArgs> for CancelSync {
  #[instrument(
    "CancelSync",
    skip_all,
    fields(
      task_id = task_id.to_string(),
      operator = user.id,
      update_id = update.id,
      sync = self.sync,
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
    let sync = get_check_permissions::<ResourceSync>(
      &self.sync,
      user,
      PermissionLevel::Execute.into(),
    )
    .await?;

    let mut update = update.clone();

    // Not an error worth failing the request over: a sync that
    // finished a moment ago is indistinguishable from one that was
    // never running, and neither is something the caller did wrong.
    match sync_cancel_cache().get(&sync.id).await {
      Some(cancel) => {
        cancel.cancel();
        update.push_simple_log(
          "Cancel Sync",
          format!("Cancellation requested for {}", sync.name),
        );
      }
      None => {
        update.push_simple_log(
          "Cancel Sync",
          format!("{} is not currently running", sync.name),
        );
      }
    }

    update.finalize();
    update_update(update.clone()).await?;
    Ok(update)
  }
}
