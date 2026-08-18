use std::pin::Pin;

use anyhow::Context;
use axum::{
  Extension, Router, extract::Path, middleware, routing::post,
};
use axum_extra::{TypedHeader, headers::ContentType};
use database::mungos::by_id::find_one_by_id;
use formatting::format_serror;
use futures_util::future::join_all;
use komodo_client::{
  api::execute::*,
  entities::{
    Operation,
    permission::PermissionLevel,
    resource::ResourceQuery,
    update::{Log, Update},
    user::User,
  },
};
use mogh_auth_server::middleware::authenticate_request;
use mogh_error::Json;
use mogh_error::JsonString;
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};
use serde_json::json;
use strum::{Display, EnumDiscriminants, VariantNames};
use typeshare::typeshare;
use uuid::Uuid;

use crate::{
  auth::KomodoAuthImpl,
  helpers::{
    query::get_all_tags,
    update::{init_execution_update, update_update},
  },
  resource::{KomodoResource, list_full_for_user_using_pattern},
  state::db_client,
};

mod action;
mod alerter;
mod application;
mod build;
mod cluster;
mod deployment;
mod maintenance;
mod procedure;
mod repo;
mod server;
mod stack;
mod swarm;
mod sync;
mod terraform;

use super::Variant;

pub struct ExecuteArgs {
  /// The task id.
  /// Unique for every '/execute' call.
  pub task_id: Uuid,
  pub user: User,
  pub update: Update,
}

#[typeshare]
#[derive(
  Serialize, Deserialize, Debug, Clone, Resolve, EnumDiscriminants,
)]
#[strum_discriminants(
  name(ExecuteRequestMethod),
  derive(Display, VariantNames)
)]
#[args(ExecuteArgs)]
#[response(JsonString)]
#[error(mogh_error::Error)]
#[serde(tag = "type", content = "params")]
pub enum ExecuteRequest {
  // ==== STACK ====
  DeployStack(DeployStack),
  BatchDeployStack(BatchDeployStack),
  DeployStackIfChanged(DeployStackIfChanged),
  BatchDeployStackIfChanged(BatchDeployStackIfChanged),
  PullStack(PullStack),
  BatchPullStack(BatchPullStack),
  StartStack(StartStack),
  RestartStack(RestartStack),
  StopStack(StopStack),
  PauseStack(PauseStack),
  UnpauseStack(UnpauseStack),
  DestroyStack(DestroyStack),
  BatchDestroyStack(BatchDestroyStack),
  RunStackService(RunStackService),

  // ==== DEPLOYMENT ====
  Deploy(Deploy),
  BatchDeploy(BatchDeploy),
  PullDeployment(PullDeployment),
  StartDeployment(StartDeployment),
  RestartDeployment(RestartDeployment),
  PauseDeployment(PauseDeployment),
  UnpauseDeployment(UnpauseDeployment),
  StopDeployment(StopDeployment),
  DestroyDeployment(DestroyDeployment),
  BatchDestroyDeployment(BatchDestroyDeployment),

  // ==== BUILD ====
  RunBuild(RunBuild),
  BatchRunBuild(BatchRunBuild),
  CancelBuild(CancelBuild),

  // ==== REPO ====
  CloneRepo(CloneRepo),
  BatchCloneRepo(BatchCloneRepo),
  PullRepo(PullRepo),
  BatchPullRepo(BatchPullRepo),
  BuildRepo(BuildRepo),
  BatchBuildRepo(BatchBuildRepo),
  CancelRepoBuild(CancelRepoBuild),

  // ==== PROCEDURE ====
  RunProcedure(RunProcedure),
  BatchRunProcedure(BatchRunProcedure),
  CancelProcedure(CancelProcedure),

  // ==== ACTION ====
  RunAction(RunAction),
  BatchRunAction(BatchRunAction),
  CancelAction(CancelAction),

  // ==== SYNC ====
  RunSync(RunSync),
  CancelSync(CancelSync),

  // ==== ALERTER ====
  TestAlerter(TestAlerter),
  SendAlert(SendAlert),

  // ==== SERVER ====
  StartContainer(StartContainer),
  RestartContainer(RestartContainer),
  PauseContainer(PauseContainer),
  UnpauseContainer(UnpauseContainer),
  StopContainer(StopContainer),
  DestroyContainer(DestroyContainer),
  StartAllContainers(StartAllContainers),
  RestartAllContainers(RestartAllContainers),
  PauseAllContainers(PauseAllContainers),
  UnpauseAllContainers(UnpauseAllContainers),
  StopAllContainers(StopAllContainers),
  PruneContainers(PruneContainers),
  DeleteNetwork(DeleteNetwork),
  PruneNetworks(PruneNetworks),
  DeleteImage(DeleteImage),
  PruneImages(PruneImages),
  DeleteVolume(DeleteVolume),
  PruneVolumes(PruneVolumes),
  PruneDockerBuilders(PruneDockerBuilders),
  PruneBuildx(PruneBuildx),
  PruneSystem(PruneSystem),

  // ==== SWARM ====
  // ==== CLUSTER ====
  DeleteClusterObject(DeleteClusterObject),
  ApplyClusterObject(ApplyClusterObject),
  RestartClusterWorkload(RestartClusterWorkload),
  RollbackClusterWorkload(RollbackClusterWorkload),
  ScaleClusterWorkload(ScaleClusterWorkload),
  CordonClusterNode(CordonClusterNode),
  UncordonClusterNode(UncordonClusterNode),
  DrainClusterNode(DrainClusterNode),
  RollbackHelmRelease(RollbackHelmRelease),
  UninstallHelmRelease(UninstallHelmRelease),
  CreateClusterPortForward(CreateClusterPortForward),
  DeleteClusterPortForward(DeleteClusterPortForward),

  // ==== APPLICATION ====
  DeployApplication(DeployApplication),
  BatchDeployApplication(BatchDeployApplication),
  DestroyApplication(DestroyApplication),
  CancelApplication(CancelApplication),
  BatchDestroyApplication(BatchDestroyApplication),
  DiffApplication(DiffApplication),
  BatchDiffApplication(BatchDiffApplication),

  // ==== TERRAFORM ====
  PlanTerraform(PlanTerraform),
  BatchPlanTerraform(BatchPlanTerraform),
  ApplyTerraform(ApplyTerraform),
  BatchApplyTerraform(BatchApplyTerraform),
  DestroyTerraform(DestroyTerraform),
  CancelTerraform(CancelTerraform),
  BatchDestroyTerraform(BatchDestroyTerraform),

  RemoveSwarmNodes(RemoveSwarmNodes),
  UpdateSwarmNode(UpdateSwarmNode),
  RemoveSwarmStacks(RemoveSwarmStacks),
  RemoveSwarmServices(RemoveSwarmServices),
  CreateSwarmConfig(CreateSwarmConfig),
  RotateSwarmConfig(RotateSwarmConfig),
  RemoveSwarmConfigs(RemoveSwarmConfigs),
  CreateSwarmSecret(CreateSwarmSecret),
  RotateSwarmSecret(RotateSwarmSecret),
  RemoveSwarmSecrets(RemoveSwarmSecrets),

  // ==== MAINTENANCE ====
  ClearRepoCache(ClearRepoCache),
  BackupCoreDatabase(BackupCoreDatabase),
  GlobalAutoUpdate(GlobalAutoUpdate),
  RotateAllServerKeys(RotateAllServerKeys),
  RotateCoreKeys(RotateCoreKeys),
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
) -> mogh_error::Result<(TypedHeader<ContentType>, String)> {
  let req: ExecuteRequest = serde_json::from_value(json!({
    "type": variant,
    "params": params,
  }))?;
  handler(user, Json(req)).await
}

async fn handler(
  Extension(user): Extension<User>,
  Json(request): Json<ExecuteRequest>,
) -> mogh_error::Result<(TypedHeader<ContentType>, String)> {
  let res = match inner_handler(request, user).await? {
    ExecutionResult::Single(update) => serde_json::to_string(&update)
      .context("Failed to serialize Update")?,
    ExecutionResult::Batch(res) => res,
  };
  Ok((TypedHeader(ContentType::json()), res))
}

#[typeshare(serialized_as = "Update")]
type BoxUpdate = Box<Update>;

pub enum ExecutionResult {
  Single(BoxUpdate),
  /// The batch contents will be pre serialized here
  Batch(String),
}

pub fn inner_handler(
  request: ExecuteRequest,
  user: User,
) -> Pin<
  Box<
    dyn std::future::Future<Output = anyhow::Result<ExecutionResult>>
      + Send,
  >,
> {
  Box::pin(async move {
    let task_id = Uuid::new_v4();

    // Need to validate no cancel is active before any update is created.
    // This ensures no double update created if Cancel is called more than once for the same request.
    build::validate_cancel_build(&request).await?;
    repo::validate_cancel_repo_build(&request).await?;

    let update = init_execution_update(&request, &user).await?;

    // This will be the case for the Batch exections,
    // they don't have their own updates.
    // The batch calls also call "inner_handler" themselves,
    // and in their case will spawn tasks, so that isn't necessary
    // here either.
    if update.operation == Operation::None {
      return Ok(ExecutionResult::Batch(
        task(task_id, request, user, update).await?,
      ));
    }

    // Spawn a task for the execution which continues
    // running after this method returns.
    let handle =
      tokio::spawn(task(task_id, request, user, update.clone()));

    // Spawns another task to monitor the first for failures,
    // and add the log to Update about it (which primary task can't do because it errored out)
    tokio::spawn({
      let update_id = update.id.clone();
      async move {
        let log = match handle.await {
          Ok(Err(e)) => {
            warn!(
              api = "Execute",
              task_id = task_id.to_string(),
              "/execute request task error: {e:#}",
            );
            Log::error("Task Error", format_serror(&e.into()))
          }
          Err(e) => {
            warn!(
              api = "Execute",
              task_id = task_id.to_string(),
              "/execute request spawn error: {e:?}",
            );
            Log::error("Spawn Error", format!("{e:#?}"))
          }
          _ => return,
        };
        let res = async {
          // Nothing to do if update was never actually created,
          // which is the case when the id is empty.
          if update_id.is_empty() {
            return Ok(());
          }
          let mut update =
            find_one_by_id(&db_client().updates, &update_id)
              .await
              .context("Failed to query to db")?
              .context("No Update exists with given id")?;
          update.logs.push(log);
          update.finalize();
          update_update(update).await
        }
        .await;

        if let Err(e) = res {
          warn!(
            api = "Execute",
            task_id = task_id.to_string(),
            update_id,
            "Failed to modify Update with task error log | {e:#}"
          );
        }
      }
    });

    Ok(ExecutionResult::Single(update.into()))
  })
}

async fn task(
  id: Uuid,
  request: ExecuteRequest,
  user: User,
  update: Update,
) -> anyhow::Result<String> {
  let method: ExecuteRequestMethod = (&request).into();

  let user_id = user.id.clone();
  let username = user.username.clone();

  info!(
    api = "Execute",
    task_id = id.to_string(),
    method = method.to_string(),
    user_id,
    username,
    "EXECUTE REQUEST",
  );

  let res = match request
    .resolve(&ExecuteArgs {
      user,
      update,
      task_id: id,
    })
    .await
  {
    Err(e) => Err(e.error),
    Ok(JsonString::Err(e)) => Err(
      anyhow::Error::from(e).context("failed to serialize response"),
    ),
    Ok(JsonString::Ok(res)) => Ok(res),
  };

  if let Err(e) = &res {
    warn!(
      api = "Execute",
      task_id = id.to_string(),
      method = method.to_string(),
      user_id,
      username,
      "EXECUTE REQUEST | ERROR: {e:#}"
    );
  }

  res
}

trait BatchExecute {
  type Resource: KomodoResource;
  fn single_request(name: String) -> ExecuteRequest;
}

#[instrument("BatchExecute", skip(user))]
async fn batch_execute<E: BatchExecute>(
  pattern: &str,
  tags: Vec<String>,
  user: &User,
) -> anyhow::Result<BatchExecutionResponse> {
  let all_tags = if tags.is_empty() {
    vec![]
  } else {
    get_all_tags(None).await?
  };

  let resources = list_full_for_user_using_pattern::<E::Resource>(
    pattern,
    ResourceQuery {
      tags,
      ..Default::default()
    },
    None,
    None,
    user,
    PermissionLevel::Execute.into(),
    &all_tags,
  )
  .await?;

  let futures = resources.into_iter().map(|resource| {
    let user = user.clone();
    async move {
      inner_handler(E::single_request(resource.name.clone()), user)
        .await
        .map(|r| {
          let ExecutionResult::Single(update) = r else {
            unreachable!()
          };
          update
        })
        .map_err(|e| BatchExecutionResponseItemErr {
          name: resource.name,
          error: e.into(),
        })
        .into()
    }
  });
  Ok(join_all(futures).await)
}

#[cfg(test)]
mod tests {
  use std::collections::HashSet;

  use komodo_client::api::execute::ExecutionVariant;
  use strum::VariantNames as _;

  use super::ExecuteRequestMethod;

  /// [Execution] is matched exhaustively (the procedure runner, the
  /// permission map), so a new execute request has to appear there or
  /// the build fails. [ExecuteRequest] is a `#[derive(Resolve)]`
  /// dispatch enum - a request whose variant is missing compiles
  /// cleanly and only fails when someone calls it. Pin the dispatch
  /// enum to the list that already fails the build, so the omission
  /// fails here instead of in production.
  ///
  /// Same hazard PeripheryRequest had; see
  /// `bin/periphery/src/api/mod.rs`
  /// `tests::cancel_execution_decodes_into_the_dispatch_enum`.
  #[test]
  fn every_execution_has_an_execute_request_variant() {
    let dispatch: HashSet<&str> =
      ExecuteRequestMethod::VARIANTS.iter().copied().collect();

    // An empty list on either side makes `missing` empty too, which
    // reads exactly like a pass. Check the guard has something to
    // guard before trusting that it found nothing.
    assert!(
      !dispatch.is_empty() && !ExecutionVariant::VARIANTS.is_empty(),
      "no variants to compare - the derive is not wired up, so this \
       test proves nothing",
    );

    let missing = ExecutionVariant::VARIANTS
      .iter()
      .copied()
      .filter(|variant| !NOT_EXECUTE_REQUESTS.contains(variant))
      .filter(|variant| !dispatch.contains(variant))
      .collect::<Vec<_>>();

    assert!(
      missing.is_empty(),
      "these Executions have no ExecuteRequest variant, so calling \
       them fails at runtime instead of at build time: {missing:?}. \
       Add the variant to ExecuteRequest in this file.",
    );
  }

  /// Executions that are deliberately not execute requests:
  /// - `None` / `Sleep` run inline in the procedure runner
  ///   (`helpers/procedure.rs`), they never go over `/execute`.
  /// - `CommitSync` is a Write request, dispatched by
  ///   `WriteRequest` (`api/write/mod.rs`).
  const NOT_EXECUTE_REQUESTS: [&str; 3] =
    ["None", "Sleep", "CommitSync"];

  /// The allowlist above is a hand-kept exception list, so it decays
  /// the moment one of its entries becomes a real execute request -
  /// and a stale exception silently un-guards that variant.
  #[test]
  fn the_allowlist_has_no_stale_entries() {
    let dispatch: HashSet<&str> =
      ExecuteRequestMethod::VARIANTS.iter().copied().collect();

    let stale = NOT_EXECUTE_REQUESTS
      .iter()
      .copied()
      .filter(|variant| dispatch.contains(variant))
      .collect::<Vec<_>>();

    assert!(
      stale.is_empty(),
      "NOT_EXECUTE_REQUESTS still excuses {stale:?}, but they are \
       ExecuteRequest variants now - drop them from the list so they \
       are guarded.",
    );

    let unknown = NOT_EXECUTE_REQUESTS
      .iter()
      .copied()
      .filter(|variant| !ExecutionVariant::VARIANTS.contains(variant))
      .collect::<Vec<_>>();

    assert!(
      unknown.is_empty(),
      "NOT_EXECUTE_REQUESTS names {unknown:?}, which are not \
       Executions at all - the list is out of date.",
    );
  }
}
