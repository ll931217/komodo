use anyhow::Context;
use axum::{
  Extension, Router, extract::Path, middleware, routing::post,
};
use komodo_client::{api::write::*, entities::user::User};
use mogh_auth_server::middleware::authenticate_request;
use mogh_error::Json;
use mogh_error::Response;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use serde_json::json;
use strum::Display;
use strum::EnumDiscriminants;
use typeshare::typeshare;
use uuid::Uuid;

use crate::auth::KomodoAuthImpl;

use super::Variant;

mod action;
mod alert;
mod alerter;
mod application;
mod build;
mod builder;
mod cluster;
mod crypto;
mod deployment;
mod onboarding;
mod permissions;
mod procedure;
mod provider;
mod repo;
mod resource;
mod server;
mod service_user;
mod stack;
mod swarm;
mod sync;
mod tag;
mod terminal;
mod terraform;
mod user;
mod user_group;
mod variable;

pub use {
  deployment::check_deployment_for_update_inner,
  stack::check_stack_for_update_inner,
};

pub struct WriteArgs {
  pub user: User,
}

#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, Resolve, EnumDiscriminants,
)]
#[strum_discriminants(name(WriteRequestMethod), derive(Display))]
#[args(WriteArgs)]
#[response(Response)]
#[error(mogh_error::Error)]
#[serde(tag = "type", content = "params")]
pub enum WriteRequest {
  // ==== RESOURCE ====
  UpdateResourceMeta(UpdateResourceMeta),

  // ==== CLUSTER ====
  CreateCluster(CreateCluster),
  CopyCluster(CopyCluster),
  DeleteCluster(DeleteCluster),
  UpdateCluster(UpdateCluster),
  RenameCluster(RenameCluster),

  // ==== APPLICATION ====
  CreateApplication(CreateApplication),
  CopyApplication(CopyApplication),
  DeleteApplication(DeleteApplication),
  UpdateApplication(UpdateApplication),
  RenameApplication(RenameApplication),

  // ==== TERRAFORM ====
  CreateTerraform(CreateTerraform),
  CopyTerraform(CopyTerraform),
  DeleteTerraform(DeleteTerraform),
  UpdateTerraform(UpdateTerraform),
  RenameTerraform(RenameTerraform),

  // ==== SWARM ====
  CreateSwarm(CreateSwarm),
  CopySwarm(CopySwarm),
  DeleteSwarm(DeleteSwarm),
  UpdateSwarm(UpdateSwarm),
  RenameSwarm(RenameSwarm),

  // ==== SERVER ====
  CreateServer(CreateServer),
  CopyServer(CopyServer),
  DeleteServer(DeleteServer),
  UpdateServer(UpdateServer),
  RenameServer(RenameServer),
  CreateNetwork(CreateNetwork),
  UpdateServerPublicKey(UpdateServerPublicKey),
  RotateServerKeys(RotateServerKeys),

  // ==== TERMINAL ====
  CreateTerminal(CreateTerminal),
  DeleteTerminal(DeleteTerminal),
  DeleteAllTerminals(DeleteAllTerminals),
  BatchDeleteAllTerminals(BatchDeleteAllTerminals),

  // ==== STACK ====
  CreateStack(CreateStack),
  CopyStack(CopyStack),
  DeleteStack(DeleteStack),
  UpdateStack(UpdateStack),
  RenameStack(RenameStack),
  WriteStackFileContents(WriteStackFileContents),
  RefreshStackCache(RefreshStackCache),
  CheckStackForUpdate(CheckStackForUpdate),
  BatchCheckStackForUpdate(BatchCheckStackForUpdate),

  // ==== DEPLOYMENT ====
  CreateDeployment(CreateDeployment),
  CopyDeployment(CopyDeployment),
  CreateDeploymentFromContainer(CreateDeploymentFromContainer),
  DeleteDeployment(DeleteDeployment),
  UpdateDeployment(UpdateDeployment),
  RenameDeployment(RenameDeployment),
  CheckDeploymentForUpdate(CheckDeploymentForUpdate),
  BatchCheckDeploymentForUpdate(BatchCheckDeploymentForUpdate),

  // ==== BUILD ====
  CreateBuild(CreateBuild),
  CopyBuild(CopyBuild),
  DeleteBuild(DeleteBuild),
  UpdateBuild(UpdateBuild),
  RenameBuild(RenameBuild),
  WriteBuildFileContents(WriteBuildFileContents),
  RefreshBuildCache(RefreshBuildCache),

  // ==== REPO ====
  CreateRepo(CreateRepo),
  CopyRepo(CopyRepo),
  DeleteRepo(DeleteRepo),
  UpdateRepo(UpdateRepo),
  RenameRepo(RenameRepo),
  RefreshRepoCache(RefreshRepoCache),

  // ==== PROCEDURE ====
  CreateProcedure(CreateProcedure),
  CopyProcedure(CopyProcedure),
  DeleteProcedure(DeleteProcedure),
  UpdateProcedure(UpdateProcedure),
  RenameProcedure(RenameProcedure),

  // ==== ACTION ====
  CreateAction(CreateAction),
  CopyAction(CopyAction),
  DeleteAction(DeleteAction),
  UpdateAction(UpdateAction),
  RenameAction(RenameAction),

  // ==== SYNC ====
  CreateResourceSync(CreateResourceSync),
  CopyResourceSync(CopyResourceSync),
  DeleteResourceSync(DeleteResourceSync),
  UpdateResourceSync(UpdateResourceSync),
  RenameResourceSync(RenameResourceSync),
  WriteSyncFileContents(WriteSyncFileContents),
  CommitSync(CommitSync),
  RefreshResourceSyncPending(RefreshResourceSyncPending),

  // ==== BUILDER ====
  CreateBuilder(CreateBuilder),
  CopyBuilder(CopyBuilder),
  DeleteBuilder(DeleteBuilder),
  UpdateBuilder(UpdateBuilder),
  RenameBuilder(RenameBuilder),

  // ==== ALERTER ====
  CreateAlerter(CreateAlerter),
  CopyAlerter(CopyAlerter),
  DeleteAlerter(DeleteAlerter),
  UpdateAlerter(UpdateAlerter),
  RenameAlerter(RenameAlerter),

  // ==== ONBOARDING KEY ====
  CreateOnboardingKey(CreateOnboardingKey),
  UpdateOnboardingKey(UpdateOnboardingKey),
  DeleteOnboardingKey(DeleteOnboardingKey),

  // ==== USER ====
  PushRecentlyViewed(PushRecentlyViewed),
  SetLastSeenUpdate(SetLastSeenUpdate),
  CreateLocalUser(CreateLocalUser),
  DeleteUser(DeleteUser),

  // ==== SERVICE USER ====
  CreateServiceUser(CreateServiceUser),
  UpdateServiceUserDescription(UpdateServiceUserDescription),
  CreateApiKeyForServiceUser(CreateApiKeyForServiceUser),
  DeleteApiKeyForServiceUser(DeleteApiKeyForServiceUser),

  // ==== USER GROUP ====
  CreateUserGroup(CreateUserGroup),
  RenameUserGroup(RenameUserGroup),
  DeleteUserGroup(DeleteUserGroup),
  AddUserToUserGroup(AddUserToUserGroup),
  RemoveUserFromUserGroup(RemoveUserFromUserGroup),
  SetUsersInUserGroup(SetUsersInUserGroup),
  SetEveryoneUserGroup(SetEveryoneUserGroup),

  // ==== PERMISSIONS ====
  UpdateUserAdmin(UpdateUserAdmin),
  UpdateUserBasePermissions(UpdateUserBasePermissions),
  UpdatePermissionOnResourceType(UpdatePermissionOnResourceType),
  UpdatePermissionOnTarget(UpdatePermissionOnTarget),

  // ==== TAG ====
  CreateTag(CreateTag),
  DeleteTag(DeleteTag),
  RenameTag(RenameTag),
  UpdateTagColor(UpdateTagColor),

  // ==== SECRETS ====
  ReencryptSecrets(ReencryptSecrets),

  // ==== VARIABLE ====
  CreateVariable(CreateVariable),
  UpdateVariableValue(UpdateVariableValue),
  UpdateVariableDescription(UpdateVariableDescription),
  UpdateVariableIsSecret(UpdateVariableIsSecret),
  DeleteVariable(DeleteVariable),

  // ==== PROVIDER ====
  CreateGitProviderAccount(CreateGitProviderAccount),
  UpdateGitProviderAccount(UpdateGitProviderAccount),
  DeleteGitProviderAccount(DeleteGitProviderAccount),
  #[serde(alias = "CreateDockerRegistryAccount")]
  CreateImageRegistryAccount(CreateImageRegistryAccount),
  #[serde(alias = "UpdateDockerRegistryAccount")]
  UpdateImageRegistryAccount(UpdateImageRegistryAccount),
  #[serde(alias = "DeleteDockerRegistryAccount")]
  DeleteImageRegistryAccount(DeleteImageRegistryAccount),

  // ==== ALERT ====
  CloseAlert(CloseAlert),
}

pub fn router() -> Router {
  Router::new()
    .route("/", post(handler))
    .route("/{variant}", post(variant_handler))
    .layer(middleware::from_fn(
      authenticate_request::<KomodoAuthImpl, true>,
    ))
}

async fn variant_handler(
  user: Extension<User>,
  Path(Variant { variant }): Path<Variant>,
  Json(params): Json<serde_json::Value>,
) -> mogh_error::Result<axum::response::Response> {
  let req: WriteRequest = serde_json::from_value(json!({
    "type": variant,
    "params": params,
  }))?;
  handler(user, Json(req)).await
}

async fn handler(
  Extension(user): Extension<User>,
  Json(request): Json<WriteRequest>,
) -> mogh_error::Result<axum::response::Response> {
  let res = tokio::spawn(task(request, user))
    .await
    .context("failure in spawned task");

  res?
}

async fn task(
  request: WriteRequest,
  user: User,
) -> mogh_error::Result<axum::response::Response> {
  let task_id = Uuid::new_v4();
  let method: WriteRequestMethod = (&request).into();

  let user_id = user.id.clone();
  let username = user.username.clone();

  if !matches!(
    request,
    WriteRequest::SetLastSeenUpdate(_)
      | WriteRequest::PushRecentlyViewed(_)
  ) {
    info!(
      task_id = task_id.to_string(),
      method = method.to_string(),
      user_id,
      username,
      "WRITE REQUEST",
    );
  }

  let res = request.resolve(&WriteArgs { user }).await;

  if let Err(e) = &res {
    warn!(
      task_id = task_id.to_string(),
      method = method.to_string(),
      user_id,
      username,
      "WRITE REQUEST | ERROR: {:#}",
      e.error
    );
  }

  res.map(|res| res.0)
}

/// Ceiling on a cascade destroy. Generous - a compose down with a
/// slow termination timeout on a large stack is legitimately minutes -
/// but finite, because the caller is waiting on it.
const CASCADE_DESTROY_TIMEOUT: std::time::Duration =
  std::time::Duration::from_secs(900);

/// Destroy what a resource deployed, before its definition is deleted.
///
/// Awaits the execution rather than spawning it: the whole point is
/// that a failed destroy aborts the delete, so the definition of
/// something still running is never lost. Permissions are the
/// execute request's own - cascading needs Execute as well as the
/// Write the delete already required.
pub async fn cascade_destroy(
  request: crate::api::execute::ExecuteRequest,
  user: &User,
) -> anyhow::Result<()> {
  let res = crate::api::execute::inner_handler(request, user.clone())
    .await
    .context(
      "Cascade destroy failed to start, so nothing was deleted",
    )?;
  let crate::api::execute::ExecutionResult::Single(update) = res
  else {
    unreachable!("cascade destroy is never a batch execution")
  };
  // Bounded: the poll underneath is an unbounded 1s loop waiting for
  // the Update to reach Complete, and this runs inside the delete
  // request's own handler. A destroy that never finalizes - Core
  // restarted mid-execution, an agent that never answers - would
  // otherwise hold the request open forever with nothing to show for
  // it. Failing loudly leaves the definition intact, which is the
  // safe end of the trade.
  let update = tokio::time::timeout(
    CASCADE_DESTROY_TIMEOUT,
    crate::helpers::update::poll_update_until_complete(&update.id),
  )
  .await
  .map_err(|_| {
    anyhow::anyhow!(
      "Cascade destroy did not finish within {}s, so nothing was deleted. Check Update {} on the resource.",
      CASCADE_DESTROY_TIMEOUT.as_secs(),
      update.id
    )
  })?
  .context(
    "Cascade destroy did not report back, so nothing was deleted",
  )?;
  if !update.success {
    anyhow::bail!(
      "Cascade destroy failed, so nothing was deleted. See Update {} for why.",
      update.id
    );
  }
  Ok(())
}
