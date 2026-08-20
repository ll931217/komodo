use komodo_client::entities::{
  SearchCombinator, TerminationSignal,
  deployment::Deployment,
  docker::{
    container::{Container, ContainerStats},
    stats::FullContainerStats,
  },
  update::Log,
};
use mogh_resolver::Resolve;
use serde::{Deserialize, Serialize};

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Container)]
#[error(anyhow::Error)]
pub struct InspectContainer {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetContainerLog {
  pub name: String,
  #[serde(default = "default_tail")]
  pub tail: u64,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

fn default_tail() -> u64 {
  50
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct GetContainerLogSearch {
  pub name: String,
  pub terms: Vec<String>,
  #[serde(default)]
  pub combinator: SearchCombinator,
  #[serde(default)]
  pub invert: bool,
  /// Enable `--timestamps`
  #[serde(default)]
  pub timestamps: bool,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(ContainerStats)]
#[error(anyhow::Error)]
pub struct GetContainerStats {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<ContainerStats>)]
#[error(anyhow::Error)]
pub struct GetContainerStatsList {}

//

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(FullContainerStats)]
#[error(anyhow::Error)]
pub struct GetFullContainerStats {
  pub name: String,
}

//

// =======
// ACTIONS
// =======

/// Executes `docker run` to create a container
/// using info given by the Deployment
///
/// Responds with every log the deploy produced: the pre-deploy hook,
/// the run itself, and whichever of post-deploy / on-fail ran. One Log
/// could not carry a hook's output, and a hook whose output is
/// discarded is a hook nobody can debug.
#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(RunContainerResponse)]
#[error(anyhow::Error)]
pub struct RunContainer {
  pub deployment: Deployment,
  pub stop_signal: Option<TerminationSignal>,
  pub stop_time: Option<i32>,
  /// Override registry token with one sent from core.
  pub registry_token: Option<String>,
  /// Propogate any secret replacers from core interpolation.
  #[serde(default)]
  pub replacers: Vec<(String, String)>,
}

/// The logs from a deploy, from either side of the hook change.
///
/// A Periphery older than the deploy hooks answers with a single Log.
/// Core is upgraded first and independently of the fleet - Periphery
/// is installed per host - so accepting both shapes is what keeps
/// `Deploy` working on hosts that have not been upgraded yet.
/// `Logs` is listed first: a JSON array can only be that variant.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum RunContainerResponse {
  Logs(Vec<Log>),
  Log(Box<Log>),
}

impl RunContainerResponse {
  pub fn into_logs(self) -> Vec<Log> {
    match self {
      RunContainerResponse::Logs(logs) => logs,
      RunContainerResponse::Log(log) => vec![*log],
    }
  }
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct StartContainer {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct RestartContainer {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct PauseContainer {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct UnpauseContainer {
  pub name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct StopContainer {
  pub name: String,
  pub signal: Option<TerminationSignal>,
  pub time: Option<i32>,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct RemoveContainer {
  pub name: String,
  pub signal: Option<TerminationSignal>,
  pub time: Option<i32>,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct RenameContainer {
  pub curr_name: String,
  pub new_name: String,
}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Log)]
#[error(anyhow::Error)]
pub struct PruneContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct StartAllContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct RestartAllContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct PauseAllContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct UnpauseAllContainers {}

//

#[derive(Serialize, Deserialize, Debug, Clone, Resolve)]
#[response(Vec<Log>)]
#[error(anyhow::Error)]
pub struct StopAllContainers {}

#[cfg(test)]
mod run_container_response_tests {
  use super::RunContainerResponse;

  /// The compatibility this enum exists for. Core is upgraded before
  /// the Periphery fleet is, so a Core that could only read the new
  /// shape would break `Deploy` on every host still on the old one -
  /// and it would break at deploy time, not at startup.
  #[test]
  fn reads_both_the_old_and_new_shapes() {
    let one = r#"{"stage":"Docker Run","command":"docker run","stdout":"ok","stderr":"","success":true,"start_ts":0,"end_ts":1}"#;
    let logs =
      serde_json::from_str::<RunContainerResponse>(one).unwrap();
    let logs = logs.into_logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].stage, "Docker Run");

    let many = format!("[{one},{one}]");
    let logs =
      serde_json::from_str::<RunContainerResponse>(&many).unwrap();
    assert_eq!(logs.into_logs().len(), 2);
  }

  /// The wire shape Core sends must be the array, not a tagged
  /// wrapper: an old Periphery is not the only reader, and a Core
  /// reading its own output has to agree with itself.
  #[test]
  fn serializes_as_a_bare_array() {
    let json =
      serde_json::to_string(&RunContainerResponse::Logs(vec![
        Default::default(),
      ]))
      .unwrap();
    assert!(json.starts_with('['), "{json}");
  }
}
