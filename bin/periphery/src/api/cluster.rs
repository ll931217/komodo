use anyhow::Context;
use command::run_komodo_standard_command;
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ClusterTarget, PollClusterStatus, PollClusterStatusResponse,
};
use tokio::fs;

use crate::config::periphery_config;

/// A kubectl invocation, plus any temporary kubeconfig it needs.
///
/// The temp file is removed when this is dropped-ish (explicitly, via
/// [ClusterCommand::cleanup]) so interpolated credentials don't linger
/// on disk longer than the command.
pub struct ClusterCommand {
  pub command: String,
  temp_kubeconfig: Option<std::path::PathBuf>,
}

impl ClusterCommand {
  /// Build `kubectl <args>` scoped to the target's kubeconfig/context.
  ///
  /// Managed kubeconfig contents are written to a private temp file
  /// under the Periphery root directory rather than passed on the
  /// command line, which would leak them into the process table.
  pub async fn build(
    target: &ClusterTarget,
    args: &str,
  ) -> anyhow::Result<ClusterCommand> {
    let mut command = String::from("kubectl");
    let mut temp_kubeconfig = None;

    if !target.kubeconfig_contents.is_empty() {
      let dir = periphery_config().root_directory.join("clusters");
      fs::create_dir_all(&dir).await.with_context(|| {
        format!("Failed to create {}", dir.display())
      })?;
      // Random name so concurrent commands don't share a file.
      let path = dir.join(format!(
        "kubeconfig-{}",
        komodo_client::entities::random_string(10)
      ));
      fs::write(&path, &target.kubeconfig_contents)
        .await
        .with_context(|| {
          format!("Failed to write kubeconfig to {}", path.display())
        })?;
      set_private(&path).await?;
      command.push_str(&format!(" --kubeconfig {}", path.display()));
      temp_kubeconfig = Some(path);
    } else if !target.kubeconfig_path.is_empty() {
      command.push_str(&format!(
        " --kubeconfig {}",
        target.kubeconfig_path
      ));
    }

    if !target.context.is_empty() {
      command.push_str(&format!(" --context {}", target.context));
    }

    command.push(' ');
    command.push_str(args);

    Ok(ClusterCommand {
      command,
      temp_kubeconfig,
    })
  }

  pub async fn cleanup(self) {
    if let Some(path) = self.temp_kubeconfig {
      let _ = fs::remove_file(path).await;
    }
  }
}

#[cfg(unix)]
async fn set_private(path: &std::path::Path) -> anyhow::Result<()> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    .await
    .context("Failed to restrict kubeconfig permissions")
}

#[cfg(not(unix))]
async fn set_private(_path: &std::path::Path) -> anyhow::Result<()> {
  Ok(())
}

impl Resolve<crate::api::Args> for PollClusterStatus {
  #[instrument("PollClusterStatus", skip_all, fields(
    context = self.target.context,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<PollClusterStatusResponse> {
    let cluster_command = ClusterCommand::build(
      &self.target,
      "version --output json --request-timeout 10s",
    )
    .await?;

    // The proxy only applies to reaching the api server, so it is set
    // per command rather than on the Periphery process.
    let command = if self.target.proxy_url.is_empty() {
      cluster_command.command.clone()
    } else {
      format!(
        "HTTPS_PROXY={} {}",
        self.target.proxy_url, cluster_command.command
      )
    };

    let log =
      run_komodo_standard_command("Poll Cluster", None, command)
        .await;
    cluster_command.cleanup().await;

    if !log.success {
      return Ok(PollClusterStatusResponse {
        reachable: false,
        version: None,
        err: Some(if log.stderr.is_empty() {
          log.stdout
        } else {
          log.stderr
        }),
      });
    }

    // `kubectl version` exits 0 while printing only clientVersion when
    // the api server cannot be reached, so a server version is the
    // actual proof of reachability - not the exit status.
    match server_version(&log.stdout) {
      Some(version) => Ok(PollClusterStatusResponse {
        reachable: true,
        version: Some(version),
        err: None,
      }),
      None => Ok(PollClusterStatusResponse {
        reachable: false,
        version: None,
        err: Some(format!(
          "kubectl reported no server version. stderr: {}",
          if log.stderr.is_empty() {
            "(empty)"
          } else {
            &log.stderr
          }
        )),
      }),
    }
  }
}

/// Pull `serverVersion.gitVersion` out of `kubectl version -o json`.
/// Absent when the api server could not be reached, which is how the
/// probe distinguishes that from kubectl simply running.
fn server_version(stdout: &str) -> Option<String> {
  serde_json::from_str::<serde_json::Value>(stdout)
    .ok()?
    .get("serverVersion")?
    .get("gitVersion")?
    .as_str()
    .map(str::to_string)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_server_version() {
    let stdout = r#"{
      "clientVersion": { "gitVersion": "v1.33.0" },
      "serverVersion": { "gitVersion": "v1.33.1" }
    }"#;
    assert_eq!(server_version(stdout), Some("v1.33.1".to_string()));
  }

  #[test]
  fn missing_server_version_is_none() {
    // Client-only output happens when the api server is unreachable
    // but kubectl still exits 0.
    let stdout = r#"{"clientVersion":{"gitVersion":"v1.33.0"}}"#;
    assert_eq!(server_version(stdout), None);
    assert_eq!(server_version("not json"), None);
  }
}
