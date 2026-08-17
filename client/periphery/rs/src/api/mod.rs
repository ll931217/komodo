use komodo_client::entities::{
  FileContents, NoData,
  config::{GitProvider, ImageRegistry},
  stack::{StackRemoteFileContents, StackServiceNames},
  update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

pub mod build;
pub mod cluster;
pub mod compose;
pub mod container;
pub mod docker;
pub mod git;
pub mod keys;
pub mod poll;
pub mod stats;
pub mod swarm;
pub mod terminal;
pub mod terraform;

//

#[derive(Deserialize, Debug, Clone)]
pub struct CoreConnectionQuery {
  /// Core host (eg demo.komo.do)
  pub core: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PeripheryConnectionQuery {
  /// Server Id or name
  pub server: String,
}

//

/// Kill the command an in-flight execution is running.
///
/// The execution id is the request channel id Core already generates
/// for every request, so Core can cancel anything it has dispatched
/// without the handler needing to report an id back first.
///
/// This arrives as its own request on the same connection, and
/// Periphery spawns every request, so it runs while the execution it
/// targets is still blocked in its command. Cancelling therefore does
/// not depend on the original request's connection state - which is
/// the point, since before this the only way to stop a command was to
/// drop the websocket and rely on `kill_on_drop`.
///
/// Kills the whole process group, not just the direct child: see
/// [command::CommandOptions].
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(NoData)]
#[error(anyhow::Error)]
pub struct CancelExecution {
  /// The id of the execution to cancel.
  pub execution_id: uuid::Uuid,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(GetHealthResponse)]
#[error(anyhow::Error)]
pub struct GetHealth {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetHealthResponse {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(GetVersionResponse)]
#[error(anyhow::Error)]
pub struct GetVersion {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetVersionResponse {
  pub version: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ListGitProvidersResponse)]
#[error(anyhow::Error)]
pub struct ListGitProviders {}

pub type ListGitProvidersResponse = Vec<GitProvider>;

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ListImageRegistriesResponse)]
#[error(anyhow::Error)]
pub struct ListImageRegistries {}

pub type ListImageRegistriesResponse = Vec<ImageRegistry>;

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<String>)]
#[error(anyhow::Error)]
pub struct ListSecrets {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct PruneSystem {}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployStackResponse {
  /// If any of the required files are missing, they will be here.
  pub missing_files: Vec<String>,
  /// The logs produced by the deploy
  pub logs: Vec<Log>,
  /// Whether stack was successfully deployed
  pub deployed: bool,
  /// The stack services.
  ///
  /// Note. The "image" is after interpolation.
  pub services: Vec<StackServiceNames>,
  /// The deploy compose file contents if they could be acquired, or empty vec.
  pub file_contents: Vec<StackRemoteFileContents>,
  /// The error in getting remote file contents at the path, or null
  pub remote_errors: Vec<FileContents>,
  /// The output of `docker compose config` / `docker stack config` at deploy time
  pub merged_config: Option<String>,
  /// If its a repo based stack, will include the latest commit hash
  pub commit_hash: Option<String>,
  /// If its a repo based stack, will include the latest commit message
  pub commit_message: Option<String>,
}
