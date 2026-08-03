use std::path::PathBuf;

use anyhow::{Context, anyhow};
use command::{
  KomodoCommandMode, run_komodo_command_with_sanitization,
  run_komodo_shell_command, run_komodo_standard_command,
};
use formatting::format_serror;
use komodo_client::entities::{
  all_logs_success, random_string, to_path_compatible_name,
  update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  cluster::{
    ApplyClusterManifests, ApplyClusterManifestsResponse,
    ApplyClusterObject, ClusterApplyMode, ClusterManifestSource,
    ClusterRolloutVerb, ClusterTarget, DeleteClusterResource,
    DrainClusterNode, GetClusterPodLog, GetClusterPodLogSearch,
    GetClusterResources, PollClusterStatus,
    PollClusterStatusResponse, RolloutClusterWorkload,
    ScaleClusterResource, SetClusterNodeSchedulable,
  },
  git::{CloneRepo, PullOrCloneRepo},
};
use tokio::fs;

use crate::{config::periphery_config, helpers::format_log_grep};

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
        expand_home(&target.kubeconfig_path)
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

  /// Build the kubectl prefix for a long-lived session.
  ///
  /// A managed kubeconfig is written to a file that is deliberately
  /// NOT removed: an interactive terminal keeps reading it for as long
  /// as the session lives, so it is cleaned up with the Periphery
  /// root directory rather than after the command.
  pub async fn build_persistent(
    target: &ClusterTarget,
    args: &str,
  ) -> anyhow::Result<String> {
    let mut command = ClusterCommand::build(target, args).await?;
    command.temp_kubeconfig = None;
    Ok(command.command)
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

/// Expand a leading `~` against the Periphery user's home.
///
/// Neither kubectl nor the standard command runner (which execs kubectl
/// directly rather than through a shell) expands `~`, so a path like
/// `~/.kube/config` would otherwise reach kubectl as a literal
/// directory name: "stat ~/.kube/config: no such file or directory".
fn expand_home(path: &str) -> String {
  let Some(rest) = path.strip_prefix('~') else {
    return path.to_string();
  };
  // `~user/...` is another user's home, not ours to resolve.
  if !rest.is_empty() && !rest.starts_with('/') {
    return path.to_string();
  }
  match std::env::var("HOME") {
    Ok(home) if !home.is_empty() => format!("{home}{rest}"),
    _ => path.to_string(),
  }
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

    let log = run_komodo_standard_command(
      "Poll Cluster",
      command,
      Default::default(),
    )
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

  #[test]
  fn parses_rollout_targets() {
    // Several resources come back as a List. `web` and `db` sit in
    // different namespaces, which is the whole point of reading them
    // back instead of parsing apply's namespace-less stdout.
    let stdout = r#"{
      "kind": "List",
      "items": [
        { "kind": "Deployment", "metadata": { "name": "web", "namespace": "front" } },
        { "kind": "Service", "metadata": { "name": "web", "namespace": "front" } },
        { "kind": "StatefulSet", "metadata": { "name": "db", "namespace": "data" } },
        { "kind": "ConfigMap", "metadata": { "name": "settings", "namespace": "front" } },
        { "kind": "DaemonSet", "metadata": { "name": "agent", "namespace": "kube-system" } }
      ]
    }"#;
    assert_eq!(
      rollout_targets(stdout).unwrap(),
      vec![
        ("front".to_string(), "deployment/web".to_string()),
        ("data".to_string(), "statefulset/db".to_string()),
        ("kube-system".to_string(), "daemonset/agent".to_string()),
      ]
    );
  }

  #[test]
  fn parses_single_rollout_target() {
    // A lone resource comes back as a bare object, not a List.
    let stdout = r#"{
      "kind": "Deployment",
      "metadata": { "name": "web", "namespace": "front" }
    }"#;
    assert_eq!(
      rollout_targets(stdout).unwrap(),
      vec![("front".to_string(), "deployment/web".to_string())]
    );
  }

  #[test]
  fn non_workloads_yield_no_rollout_targets() {
    let stdout = r#"{
      "kind": "Namespace",
      "metadata": { "name": "foo" }
    }"#;
    assert!(rollout_targets(stdout).unwrap().is_empty());
    // Unparseable output is an error, never an empty wait list: a
    // green Deploy that checked nothing is worse than a failed one.
    assert!(rollout_targets("not json").is_err());
  }

  #[test]
  fn expands_leading_home_tilde() {
    let home = std::env::var("HOME").expect("HOME is set");
    assert_eq!(
      expand_home("~/.kube/config"),
      format!("{home}/.kube/config")
    );
    assert_eq!(expand_home("~"), home);
    // Left alone: another user's home, absolute paths, relative paths.
    assert_eq!(
      expand_home("~other/.kube/config"),
      "~other/.kube/config"
    );
    assert_eq!(
      expand_home("/etc/rancher/k3s/k3s.yaml"),
      "/etc/rancher/k3s/k3s.yaml"
    );
    assert_eq!(expand_home("kube/config"), "kube/config");
  }
}

impl Resolve<crate::api::Args> for ApplyClusterManifests {
  #[instrument("ApplyClusterManifests", skip_all, fields(
    namespace = self.namespace,
    mode = format!("{:?}", self.mode),
  ))]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<ApplyClusterManifestsResponse> {
    let mut res = ApplyClusterManifestsResponse::default();

    // Materialize the manifests, whatever they came from, into a
    // directory plus the paths to apply within it.
    let materialized =
      match write_manifests(&self, &mut res, args).await {
        Ok(materialized) => materialized,
        Err(e) => {
          res.logs.push(Log::error(
            "Write Manifests",
            format_serror(&e.into()),
          ));
          return Ok(res);
        }
      };
    // A failed clone leaves logs but nothing to apply.
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    let result = apply(&self, &materialized, &mut res).await;
    materialized.cleanup().await;
    result?;

    Ok(res)
  }
}

/// A directory of manifests ready for kubectl, and whether it is ours
/// to delete afterwards.
struct Materialized {
  directory: PathBuf,
  /// Paths within `directory` to apply. Empty applies the directory.
  file_paths: Vec<String>,
  /// Only true for manifests Komodo wrote for this one command; a
  /// cloned repo and host files are left alone.
  temporary: bool,
}

impl Materialized {
  async fn cleanup(self) {
    if self.temporary {
      let _ = fs::remove_dir_all(&self.directory).await;
    }
  }
}

/// The Cluster equivalent of write_stack: get manifests onto disk.
///
/// Deliberately separate from the Stack version rather than
/// generalizing it: that one also handles env files, compose services
/// and remote file reporting, none of which apply here.
async fn write_manifests(
  req: &ApplyClusterManifests,
  res: &mut ApplyClusterManifestsResponse,
  args: &crate::api::Args,
) -> anyhow::Result<Materialized> {
  match &req.source {
    ClusterManifestSource::Contents(manifests) => {
      // kustomize needs a kustomization.yaml in the directory; plain
      // manifests go to a single file.
      let dir = periphery_config()
        .root_directory
        .join("clusters")
        .join(format!("manifests-{}", random_string(10)));
      fs::create_dir_all(&dir).await.with_context(|| {
        format!("Failed to create {}", dir.display())
      })?;
      let file_name = if req.kustomize {
        "kustomization.yaml"
      } else {
        "manifests.yaml"
      };
      let path = dir.join(file_name);
      fs::write(&path, manifests).await.with_context(|| {
        format!("Failed to write {}", path.display())
      })?;
      set_private(&path).await?;
      Ok(Materialized {
        directory: dir,
        file_paths: vec![file_name.to_string()],
        temporary: true,
      })
    }

    ClusterManifestSource::FilesOnHost {
      run_directory,
      file_paths,
    } => {
      let directory = PathBuf::from(run_directory)
        .components()
        .collect::<PathBuf>();
      if !directory.is_dir() {
        return Err(anyhow!(
          "Manifest directory {} does not exist on this host",
          directory.display()
        ));
      }
      Ok(Materialized {
        directory,
        file_paths: file_paths.clone(),
        temporary: false,
      })
    }

    ClusterManifestSource::Repo {
      args: repo_args,
      git_token,
      reclone,
      run_directory,
      file_paths,
    } => {
      let root = periphery_config()
        .repo_dir()
        .join(to_path_compatible_name(&repo_args.name))
        .components()
        .collect::<PathBuf>();

      let mut repo_args = repo_args.clone();
      repo_args.destination = Some(root.display().to_string());

      let clone = if *reclone {
        CloneRepo {
          args: repo_args,
          git_token: git_token.clone(),
          environment: Default::default(),
          env_file_path: Default::default(),
          on_clone: Default::default(),
          on_pull: Default::default(),
          skip_secret_interp: Default::default(),
          replacers: Default::default(),
        }
        .resolve(args)
        .await
      } else {
        PullOrCloneRepo {
          args: repo_args,
          git_token: git_token.clone(),
          environment: Default::default(),
          env_file_path: Default::default(),
          on_clone: Default::default(),
          on_pull: Default::default(),
          skip_secret_interp: Default::default(),
          replacers: Default::default(),
        }
        .resolve(args)
        .await
      }
      .map_err(|e| anyhow!("{e:#}"))?;

      res.logs.extend(clone.res.logs);
      res.commit_hash = clone.res.commit_hash;
      res.commit_message = clone.res.commit_message;

      Ok(Materialized {
        directory: root
          .join(run_directory)
          .components()
          .collect::<PathBuf>(),
        file_paths: file_paths.clone(),
        temporary: false,
      })
    }
  }
}

async fn apply(
  req: &ApplyClusterManifests,
  materialized: &Materialized,
  res: &mut ApplyClusterManifestsResponse,
) -> anyhow::Result<()> {
  let verb = match req.mode {
    ClusterApplyMode::Apply => "apply",
    ClusterApplyMode::Delete => "delete",
    ClusterApplyMode::Diff => "diff",
  };

  // kustomize takes the directory; otherwise each path is a -f, and an
  // empty list means the whole directory.
  let source = if req.kustomize {
    format!("-k {}", materialized.directory.display())
  } else if materialized.file_paths.is_empty() {
    format!("-f {}", materialized.directory.display())
  } else {
    materialized
      .file_paths
      .iter()
      .map(|path| {
        format!("-f {}", materialized.directory.join(path).display())
      })
      .collect::<Vec<_>>()
      .join(" ")
  };

  let mut kubectl_args =
    format!("{verb} {source} --namespace {}", req.namespace);
  if req.mode == ClusterApplyMode::Delete {
    // Destroying something already gone is not a failure.
    kubectl_args.push_str(" --ignore-not-found=true");
  }
  for extra in &req.extra_args {
    kubectl_args.push(' ');
    kubectl_args.push_str(extra);
  }

  let cluster_command =
    ClusterCommand::build(&req.target, &kubectl_args).await?;
  let command = with_proxy(&req.target, &cluster_command.command);

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
  let log = run_komodo_command_with_sanitization(
    stage,
    command,
    Default::default(),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await;
  cluster_command.cleanup().await;
  res.logs.extend(log);

  // The api server accepting manifests says nothing about the pods
  // actually coming up. When asked, block on each applied workload's
  // rollout so a crashlooping deploy fails the Update.
  if req.wait_ready
    && req.mode == ClusterApplyMode::Apply
    && all_logs_success(&res.logs)
  {
    let targets = match applied_workloads(req, &source).await {
      Ok(targets) => targets,
      Err(e) => {
        // Never fall through to "nothing to wait on": that would
        // report a green Deploy having checked nothing.
        res.logs.push(Log::error(
          "Wait For Rollout",
          format_serror(
            &e.context("Failed to resolve applied workloads").into(),
          ),
        ));
        return Ok(());
      }
    };
    for (namespace, target) in targets {
      let args = format!(
        "rollout status {target} --namespace {namespace} --timeout 120s"
      );
      let cluster_command =
        ClusterCommand::build(&req.target, &args).await?;
      let command = with_proxy(&req.target, &cluster_command.command);
      let log = run_komodo_standard_command(
        "Wait For Rollout",
        command,
        Default::default(),
      )
      .await;
      cluster_command.cleanup().await;
      let failed = !log.success;
      res.logs.push(log);
      if failed {
        break;
      }
    }
  }

  Ok(())
}

/// Ask the cluster for the objects the manifests just applied.
async fn applied_workloads(
  req: &ApplyClusterManifests,
  source: &str,
) -> anyhow::Result<Vec<(String, String)>> {
  let args = format!(
    "get {source} --namespace {} --output json",
    req.namespace
  );
  let cluster_command =
    ClusterCommand::build(&req.target, &args).await?;
  let command = with_proxy(&req.target, &cluster_command.command);
  let log = run_komodo_standard_command(
    "Resolve Workloads",
    command,
    Default::default(),
  )
  .await;
  cluster_command.cleanup().await;
  if !log.success {
    anyhow::bail!("kubectl get failed: {}", log.stderr);
  }
  rollout_targets(&log.stdout)
}

/// The workloads to wait on, as `(namespace, kind/name)`.
///
/// Read out of `kubectl get -o json` rather than apply's stdout:
/// apply prints `deployment.apps/foo configured` with no namespace, so
/// a manifest set spanning namespaces would be waited on in the wrong
/// one. Asking kubectl for the objects instead gets the namespace it
/// actually resolved, whether from the manifest or the --namespace
/// default.
///
/// `kubectl get` returns a bare object for a single resource and a
/// List for several, so both shapes are accepted.
fn rollout_targets(
  get_stdout: &str,
) -> anyhow::Result<Vec<(String, String)>> {
  let json: serde_json::Value = serde_json::from_str(get_stdout)
    .context("Failed to parse kubectl get json")?;
  let objects = match json.get("items") {
    Some(serde_json::Value::Array(items)) => items.as_slice(),
    _ => std::slice::from_ref(&json),
  };
  Ok(
    objects
      .iter()
      .filter_map(|object| {
        let kind = match object.get("kind")?.as_str()? {
          "Deployment" => "deployment",
          "StatefulSet" => "statefulset",
          "DaemonSet" => "daemonset",
          _ => return None,
        };
        let metadata = object.get("metadata")?;
        Some((
          metadata.get("namespace")?.as_str()?.to_string(),
          format!("{kind}/{}", metadata.get("name")?.as_str()?),
        ))
      })
      .collect(),
  )
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
    let log = run_komodo_standard_command(
      "Get Resources",
      command,
      Default::default(),
    )
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
    let log = run_komodo_standard_command(
      "Delete Resource",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for RolloutClusterWorkload {
  #[instrument("RolloutClusterWorkload", skip_all, fields(
    verb = format!("{:?}", self.verb),
    kind = self.kind,
    namespace = self.namespace,
    name = self.name,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let (verb, stage) = match self.verb {
      ClusterRolloutVerb::Restart => ("restart", "Rollout Restart"),
      ClusterRolloutVerb::Undo => ("undo", "Rollout Undo"),
    };
    let mut args =
      format!("rollout {verb} {}/{}", self.kind, self.name);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log =
      run_komodo_standard_command(stage, command, Default::default())
        .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for ScaleClusterResource {
  #[instrument("ScaleClusterResource", skip_all, fields(
    kind = self.kind,
    namespace = self.namespace,
    name = self.name,
    replicas = self.replicas,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!(
      "scale {}/{} --replicas {}",
      self.kind, self.name, self.replicas
    );
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Scale",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for SetClusterNodeSchedulable {
  #[instrument("SetClusterNodeSchedulable", skip_all, fields(
    node = self.node,
    schedulable = self.schedulable,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let (verb, stage) = if self.schedulable {
      ("uncordon", "Uncordon Node")
    } else {
      ("cordon", "Cordon Node")
    };
    let args = format!("{verb} {}", self.node);

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log =
      run_komodo_standard_command(stage, command, Default::default())
        .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for DrainClusterNode {
  #[instrument("DrainClusterNode", skip_all, fields(
    node = self.node,
    force = self.force,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    // DaemonSet pods cannot be evicted and every real node runs some,
    // so the flag is not optional in practice.
    let mut args = format!("drain {} --ignore-daemonsets", self.node);
    if self.force {
      args.push_str(" --force");
    }
    if self.delete_emptydir_data {
      args.push_str(" --delete-emptydir-data");
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Drain Node",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for ApplyClusterObject {
  #[instrument("ApplyClusterObject", skip_all, fields(
    namespace = self.namespace,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    // Written to a private temp file like managed kubeconfigs, so the
    // manifest never rides the command line.
    let dir = periphery_config().root_directory.join("clusters");
    fs::create_dir_all(&dir).await.with_context(|| {
      format!("Failed to create {}", dir.display())
    })?;
    let path = dir.join(format!("object-{}.yaml", random_string(10)));
    fs::write(&path, &self.contents).await.with_context(|| {
      format!("Failed to write manifest to {}", path.display())
    })?;
    set_private(&path).await?;

    let mut args = format!("apply -f {}", path.display());
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Apply Object",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;
    let _ = fs::remove_file(&path).await;

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

impl Resolve<crate::api::Args> for GetClusterPodLog {
  #[instrument("GetClusterPodLog", skip_all, fields(
    namespace = self.namespace,
    pod = self.pod,
    container = self.container.as_deref().unwrap_or("-"),
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!("logs {}", self.pod);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    if let Some(container) = &self.container {
      args.push_str(&format!(" --container {container}"));
    }
    args.push_str(&format!(" --tail {}", self.tail));
    if self.previous {
      args.push_str(" --previous");
    }
    if self.timestamps {
      args.push_str(" --timestamps");
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Pod Log",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}

impl Resolve<crate::api::Args> for GetClusterPodLogSearch {
  #[instrument("GetClusterPodLogSearch", skip_all, fields(
    namespace = self.namespace,
    pod = self.pod,
    container = self.container.as_deref().unwrap_or("-"),
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!("logs {}", self.pod);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    if let Some(container) = &self.container {
      args.push_str(&format!(" --container {container}"));
    }
    // Match the container log search: grep over a bounded tail.
    args.push_str(" --tail 5000");
    if self.timestamps {
      args.push_str(" --timestamps");
    }

    let grep =
      format_log_grep(&self.terms, self.combinator, self.invert);

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = format!(
      "{} 2>&1 | {grep}",
      with_proxy(&self.target, &cluster_command.command)
    );
    let log = run_komodo_shell_command(
      "Pod Log Grep",
      &command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;

    Ok(log)
  }
}
