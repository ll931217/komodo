use anyhow::Context as _;
use command::{CommandOptions, run_komodo_standard_command};
use encoding::{EncodedJsonMessage, EncodedResponse};
use komodo_client::entities::{
  NoData,
  config::{GitProvider, ImageRegistry},
  stats::SystemProcess,
  update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  build::*, cluster::*, compose::*, container::*, docker::*, git::*,
  keys::*, poll::*, stats::*, swarm::*, terminal::*, terraform::*, *,
};
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{config::periphery_config, state::stats_client};

pub mod terminal;

mod build;
mod cluster;
mod compose;
mod container;
mod docker;
mod git;
mod keys;
mod poll;
mod swarm;
mod terraform;

#[derive(Debug)]
pub struct Args {
  pub core: String,
  /// The execution id.
  /// Unique for every /execute call.
  pub id: Uuid,
  /// Fired when [periphery_client::api::CancelExecution] names this
  /// execution.
  ///
  /// It lives on Args, rather than being looked up from the cancel
  /// cache at each call site, so that making a command interruptible
  /// is `.cancel(args.cancel.clone())` and nothing else - a lookup
  /// that can be got wrong is a lookup that will be.
  pub cancel: CancellationToken,
}

#[derive(
  Serialize, Deserialize, Debug, Clone, Resolve, EnumDiscriminants,
)]
#[strum_discriminants(name(PeripheryRequestVariant))]
#[args(Args)]
#[response(EncodedResponse<EncodedJsonMessage>)]
#[error(anyhow::Error)]
#[serde(tag = "type", content = "params")]
#[allow(clippy::enum_variant_names, clippy::large_enum_variant)]
pub enum PeripheryRequest {
  // Stats / Info (Read)
  PollStatus(PollStatus),
  GetHealth(GetHealth),
  GetVersion(GetVersion),
  GetSystemProcesses(GetSystemProcesses),
  GetLatestCommit(GetLatestCommit),

  // Config (Read)
  ListGitProviders(ListGitProviders),
  ListImageRegistries(ListImageRegistries),
  ListSecrets(ListSecrets),

  // Repo (Write)
  CloneRepo(CloneRepo),
  PullRepo(PullRepo),
  PullOrCloneRepo(PullOrCloneRepo),
  RenameRepo(RenameRepo),
  DeleteRepo(DeleteRepo),

  // Build
  GetDockerfileContentsOnHost(GetDockerfileContentsOnHost),
  WriteDockerfileContentsToHost(WriteDockerfileContentsToHost),
  Build(Build),
  CancelBuild(CancelBuild),
  CancelExecution(CancelExecution),
  PruneBuilders(PruneBuilders),
  PruneBuildx(PruneBuildx),

  // Compose (Read)
  GetComposeContentsOnHost(GetComposeContentsOnHost),
  GetComposeLog(GetComposeLog),
  GetComposeLogSearch(GetComposeLogSearch),

  // Compose (Write)
  WriteComposeContentsToHost(WriteComposeContentsToHost),
  WriteCommitComposeContents(WriteCommitComposeContents),
  ComposePull(ComposePull),
  ComposeUp(ComposeUp),
  ComposeDown(ComposeDown),
  ComposeExecution(ComposeExecution),
  ComposeRun(ComposeRun),

  // Container (Read)
  InspectContainer(InspectContainer),
  GetContainerLog(GetContainerLog),
  GetContainerLogSearch(GetContainerLogSearch),
  GetContainerStats(GetContainerStats),
  GetContainerStatsList(GetContainerStatsList),
  GetFullContainerStats(GetFullContainerStats),

  // Container (Write)
  RunContainer(RunContainer),
  StartContainer(StartContainer),
  RestartContainer(RestartContainer),
  PauseContainer(PauseContainer),
  UnpauseContainer(UnpauseContainer),
  StopContainer(StopContainer),
  StartAllContainers(StartAllContainers),
  RestartAllContainers(RestartAllContainers),
  PauseAllContainers(PauseAllContainers),
  UnpauseAllContainers(UnpauseAllContainers),
  StopAllContainers(StopAllContainers),
  RemoveContainer(RemoveContainer),
  RenameContainer(RenameContainer),
  PruneContainers(PruneContainers),

  // Networks (Read)
  InspectNetwork(InspectNetwork),

  // Networks (Write)
  CreateNetwork(CreateNetwork),
  DeleteNetwork(DeleteNetwork),
  PruneNetworks(PruneNetworks),

  // Image (Read)
  InspectImage(InspectImage),
  ImageHistory(ImageHistory),
  GetLatestImageDigest(GetLatestImageDigest),

  // Image (Write)
  PullImage(PullImage),
  DeleteImage(DeleteImage),
  PruneImages(PruneImages),

  // Volume (Read)
  InspectVolume(InspectVolume),

  // Volume (Write)
  DeleteVolume(DeleteVolume),
  PruneVolumes(PruneVolumes),

  // All in one (Write)
  PruneSystem(PruneSystem),

  // Cluster (Read)
  PollClusterStatus(PollClusterStatus),

  GetClusterResources(GetClusterResources),
  GetClusterTop(GetClusterTop),
  GetClusterPodLog(GetClusterPodLog),
  GetClusterPodLogSearch(GetClusterPodLogSearch),
  ListHelmReleases(ListHelmReleases),
  InspectHelmRelease(InspectHelmRelease),
  ListClusterPortForwards(ListClusterPortForwards),

  // Cluster (Write)
  ApplyClusterManifests(ApplyClusterManifests),
  DeleteClusterResource(DeleteClusterResource),
  ApplyClusterObject(ApplyClusterObject),
  RolloutClusterWorkload(RolloutClusterWorkload),
  ScaleClusterResource(ScaleClusterResource),
  SetClusterNodeSchedulable(SetClusterNodeSchedulable),
  DrainClusterNode(DrainClusterNode),
  RollbackHelmRelease(RollbackHelmRelease),
  UninstallHelmRelease(UninstallHelmRelease),
  CreateClusterPortForward(CreateClusterPortForward),
  DeleteClusterPortForward(DeleteClusterPortForward),

  // Terraform (Write)
  RunTerraform(RunTerraform),

  // Swarm (Read)
  PollSwarmStatus(PollSwarmStatus),
  InspectSwarmNode(InspectSwarmNode),
  InspectSwarmStack(InspectSwarmStack),
  InspectSwarmService(InspectSwarmService),
  GetSwarmServiceLog(GetSwarmServiceLog),
  GetSwarmServiceLogSearch(GetSwarmServiceLogSearch),
  InspectSwarmTask(InspectSwarmTask),
  InspectSwarmConfig(InspectSwarmConfig),
  InspectSwarmSecret(InspectSwarmSecret),

  // Swarm (Write)
  UpdateSwarmNode(UpdateSwarmNode),
  RemoveSwarmNodes(RemoveSwarmNodes),
  DeploySwarmStack(DeploySwarmStack),
  RemoveSwarmStacks(RemoveSwarmStacks),
  CreateSwarmService(CreateSwarmService),
  UpdateSwarmService(UpdateSwarmService),
  RollbackSwarmService(RollbackSwarmService),
  RemoveSwarmServices(RemoveSwarmServices),
  CreateSwarmConfig(CreateSwarmConfig),
  RotateSwarmConfig(RotateSwarmConfig),
  RemoveSwarmConfigs(RemoveSwarmConfigs),
  CreateSwarmSecret(CreateSwarmSecret),
  RotateSwarmSecret(RotateSwarmSecret),
  RemoveSwarmSecrets(RemoveSwarmSecrets),

  // Terminal
  ListTerminals(ListTerminals),
  CreateServerTerminal(CreateServerTerminal),
  CreateContainerExecTerminal(CreateContainerExecTerminal),
  CreateClusterPodExecTerminal(CreateClusterPodExecTerminal),
  CreateContainerAttachTerminal(CreateContainerAttachTerminal),
  DeleteTerminal(DeleteTerminal),
  DeleteAllTerminals(DeleteAllTerminals),
  ConnectTerminal(ConnectTerminal),
  DisconnectTerminal(DisconnectTerminal),
  ExecuteTerminal(ExecuteTerminal),

  // Keys
  RotatePrivateKey(RotatePrivateKey),
  RotateCorePublicKey(RotateCorePublicKey),
}

//

impl Resolve<Args> for CancelExecution {
  #[instrument(
    "CancelExecution",
    skip_all,
    fields(
      target = self.execution_id.to_string(),
      id = args.id.to_string(),
      core = args.core,
    )
  )]
  async fn resolve(self, args: &Args) -> anyhow::Result<NoData> {
    // Not found means the execution already finished, or never
    // existed. Erroring rather than returning Ok keeps "I cancelled
    // it" from being reported for a command that ran to completion -
    // the caller can treat it as benign, but it must not be silent.
    crate::state::execution_cancel_cache()
      .get(&self.execution_id)
      .await
      .with_context(|| {
        format!(
          "No in-flight execution {} to cancel",
          self.execution_id
        )
      })?
      .cancel();
    Ok(NoData {})
  }
}

//

impl Resolve<Args> for GetHealth {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<GetHealthResponse> {
    Ok(GetHealthResponse {})
  }
}

//

impl Resolve<Args> for GetVersion {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<GetVersionResponse> {
    Ok(GetVersionResponse {
      version: env!("CARGO_PKG_VERSION").to_string(),
    })
  }
}

//

impl Resolve<Args> for GetSystemProcesses {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<SystemProcess>> {
    Ok(stats_client().read().await.get_processes())
  }
}

//

impl Resolve<Args> for ListGitProviders {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<GitProvider>> {
    Ok(periphery_config().git_providers.0.clone())
  }
}

impl Resolve<Args> for ListImageRegistries {
  async fn resolve(
    self,
    _: &Args,
  ) -> anyhow::Result<Vec<ImageRegistry>> {
    Ok(periphery_config().image_registries.0.clone())
  }
}

//

impl Resolve<Args> for ListSecrets {
  async fn resolve(self, _: &Args) -> anyhow::Result<Vec<String>> {
    Ok(
      periphery_config()
        .secrets
        .keys()
        .cloned()
        .collect::<Vec<_>>(),
    )
  }
}

impl Resolve<Args> for PruneSystem {
  #[instrument(
    "PruneSystem",
    skip_all,
    fields(
      id = args.id.to_string(),
      core = args.core
    )
  )]
  async fn resolve(self, args: &Args) -> anyhow::Result<Log> {
    let command = String::from("docker system prune -a -f --volumes");
    Ok(
      run_komodo_standard_command(
        "Prune System",
        command,
        CommandOptions::default(),
      )
      .await,
    )
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;

  /// Adding a request struct and its `Resolve` impl compiles fine
  /// without the [PeripheryRequest] variant. Core also compiles fine
  /// sending it, because `PeripheryClient::request` is generic over
  /// the struct and never mentions this enum. The only symptom of the
  /// missing variant is a warn-log on Periphery and a timeout on
  /// Core, at runtime, in production.
  ///
  /// So this asserts the one thing the compiler will not: that the
  /// bytes Core puts on the wire decode into a variant that exists.
  #[test]
  fn cancel_execution_decodes_into_the_dispatch_enum() {
    let execution_id = Uuid::new_v4();
    let wire = serde_json::json!({
      "type": "CancelExecution",
      "params": { "execution_id": execution_id },
    });

    let request: PeripheryRequest = serde_json::from_value(wire)
      .expect(
        "CancelExecution did not decode - is the variant missing from \
         PeripheryRequest? Nothing else would have caught that.",
      );

    match request {
      PeripheryRequest::CancelExecution(request) => {
        assert_eq!(request.execution_id, execution_id)
      }
      other => panic!("decoded into the wrong variant: {other:?}"),
    }
  }

  /// The type tag Core sends is `T::req_type()`, which the derive
  /// generates from the struct name. The enum matches on its variant
  /// name. Nothing checks that those two strings agree, so renaming
  /// either one alone silently breaks dispatch.
  #[test]
  fn the_wire_tag_matches_the_variant_name() {
    use mogh_resolver::HasResponse as _;
    assert_eq!(CancelExecution::req_type(), "CancelExecution");
  }

  #[tokio::test]
  async fn cancels_a_running_command_by_execution_id() {
    let marker = "sleep 51337";
    let execution_id = Uuid::new_v4();
    let cancel = CancellationToken::new();

    // Stands in for connection::handle_request, which registers this
    // for every request.
    crate::state::execution_cancel_cache()
      .insert(execution_id, cancel.clone())
      .await;

    tokio::spawn(async move {
      tokio::time::sleep(Duration::from_millis(300)).await;
      CancelExecution { execution_id }
        .resolve(&Args {
          core: "test".to_string(),
          // A DIFFERENT execution: the cancelling request is not the
          // one being cancelled, which is the whole point.
          id: Uuid::new_v4(),
          cancel: CancellationToken::new(),
        })
        .await
        .expect("CancelExecution should find the registered token");
    });

    let out = command::run_shell_command(
      &format!("{marker} & sleep 51336"),
      CommandOptions::default().cancel(cancel),
    )
    .await;

    assert!(
      !out.success(),
      "command should have been killed, got: {out:?}"
    );
    assert!(
      out.stderr.contains("cancelled"),
      "expected a cancellation message, got: {out:?}"
    );

    crate::state::execution_cancel_cache()
      .remove(&execution_id)
      .await;
  }

  #[tokio::test]
  async fn cancelling_an_unknown_execution_is_an_error() {
    // Returning Ok here would let Core report "cancelled" for a
    // command that actually ran to completion.
    let result = CancelExecution {
      execution_id: Uuid::new_v4(),
    }
    .resolve(&Args {
      core: "test".to_string(),
      id: Uuid::new_v4(),
      cancel: CancellationToken::new(),
    })
    .await;
    assert!(result.is_err());
  }
}
