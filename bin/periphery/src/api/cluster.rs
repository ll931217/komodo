use anyhow::Context;
use anyhow::anyhow;
use command::{
  KomodoCommandMode, run_komodo_command_with_sanitization,
  run_komodo_standard_command,
};
use komodo_client::entities::{random_string, update::Log};
use mogh_resolver::Resolve;
use periphery_client::api::cluster::{
  ApplyClusterManifests, ClusterApplyMode, ClusterTarget,
  DeleteClusterResource, GetClusterResources, PollClusterStatus,
  PollClusterStatusResponse,
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
      let path =
        dir.join(format!("kubeconfig-{}", random_string(10)));
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

impl Resolve<crate::api::Args> for ApplyClusterManifests {
  #[instrument("ApplyClusterManifests", skip_all, fields(
    namespace = self.namespace,
    mode = format!("{:?}", self.mode),
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Vec<Log>> {
    // Manifests go to a private directory so kustomize can resolve
    // relative paths, and so nothing sensitive reaches the argv.
    let dir = periphery_config()
      .root_directory
      .join("clusters")
      .join(format!("manifests-{}", random_string(10)));
    fs::create_dir_all(&dir).await.with_context(|| {
      format!("Failed to create {}", dir.display())
    })?;
    let result = apply(&self, &dir).await;
    let _ = fs::remove_dir_all(&dir).await;
    result
  }
}

async fn apply(
  req: &ApplyClusterManifests,
  dir: &std::path::Path,
) -> anyhow::Result<Vec<Log>> {
  // kustomize expects a kustomization.yaml in the directory;
  // plain manifests are applied from a single file.
  let file_name = if req.kustomize {
    "kustomization.yaml"
  } else {
    "manifests.yaml"
  };
  let path = dir.join(file_name);
  fs::write(&path, &req.manifests)
    .await
    .with_context(|| format!("Failed to write {}", path.display()))?;
  set_private(&path).await?;

  let verb = match req.mode {
    ClusterApplyMode::Apply => "apply",
    ClusterApplyMode::Delete => "delete",
    ClusterApplyMode::Diff => "diff",
  };
  let source = if req.kustomize {
    format!("-k {}", dir.display())
  } else {
    format!("-f {}", path.display())
  };
  let mut args =
    format!("{verb} {source} --namespace {}", req.namespace);
  if req.mode == ClusterApplyMode::Delete {
    // A Destroy of something already gone is not a failure.
    args.push_str(" --ignore-not-found=true");
  }
  for extra in &req.extra_args {
    args.push(' ');
    args.push_str(extra);
  }

  let cluster_command =
    ClusterCommand::build(&req.target, &args).await?;
  let command = if req.target.proxy_url.is_empty() {
    cluster_command.command.clone()
  } else {
    format!(
      "HTTPS_PROXY={} {}",
      req.target.proxy_url, cluster_command.command
    )
  };

  // `kubectl diff` exits 1 to mean "differences found", which is a
  // successful diff, so only a code above 1 is a real failure.
  let command = if req.mode == ClusterApplyMode::Diff {
    format!(
      "{command}; code=$?; if [ $code -gt 1 ]; then exit $code; fi"
    )
  } else {
    command
  };

  let stage = match req.mode {
    ClusterApplyMode::Apply => "Deploy",
    ClusterApplyMode::Delete => "Destroy",
    ClusterApplyMode::Diff => "Diff",
  };
  let replacers = req.secret_replacers.clone();
  let log = run_komodo_command_with_sanitization(
    stage,
    None,
    command,
    KomodoCommandMode::Shell,
    &replacers,
  )
  .await;
  cluster_command.cleanup().await;

  Ok(log.into_iter().collect())
}

impl Resolve<crate::api::Args> for GetClusterResources {
  #[instrument("GetClusterResources", skip_all, fields(
    kind = self.kind,
    namespace = self.namespace,
    name = self.name.as_deref().unwrap_or("*"),
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<serde_json::Value> {
    let mut args = format!("get {}", self.kind);
    if let Some(name) = &self.name {
      args.push(' ');
      args.push_str(name);
    }
    if self.all_namespaces {
      args.push_str(" --all-namespaces");
    } else if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    args.push_str(" --output json");

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log =
      run_komodo_standard_command("Get Resources", None, command)
        .await;
    cluster_command.cleanup().await;

    if !log.success {
      return Err(anyhow!(
        "{}",
        if log.stderr.is_empty() {
          log.stdout
        } else {
          log.stderr
        }
      ));
    }

    serde_json::from_str(&log.stdout)
      .context("kubectl returned output that is not valid json")
  }
}

impl Resolve<crate::api::Args> for DeleteClusterResource {
  #[instrument("DeleteClusterResource", skip_all, fields(
    kind = self.kind,
    namespace = self.namespace,
    name = self.name,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!("delete {} {}", self.kind, self.name);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log =
      run_komodo_standard_command("Delete Resource", None, command)
        .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

/// The proxy applies only to reaching the api server, so it is set per
/// command rather than on the Periphery process.
fn with_proxy(target: &ClusterTarget, command: &str) -> String {
  if target.proxy_url.is_empty() {
    command.to_string()
  } else {
    format!("HTTPS_PROXY={} {command}", target.proxy_url)
  }
}
