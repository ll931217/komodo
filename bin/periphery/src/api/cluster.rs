use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use command::{
  CommandOptions, KomodoCommandMode,
  run_komodo_command_with_sanitization, run_komodo_shell_command,
  run_komodo_standard_command,
};
use formatting::format_serror;
use komodo_client::entities::{
  all_logs_success,
  cluster::{
    ClusterMetricsEntry, ClusterMetricsKind, ClusterPortForward,
    ManifestPolicy,
  },
  random_string, to_path_compatible_name,
  update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  cluster::{
    ApplyClusterManifests, ApplyClusterManifestsResponse,
    ApplyClusterObject, ClusterApplyMode, ClusterManifestSource,
    ClusterObjectMode, ClusterRolloutVerb, ClusterTarget,
    CreateClusterPortForward, DeleteClusterPortForward,
    DeleteClusterResource, DrainClusterNode, ExecClusterPod,
    GetClusterDescribe, GetClusterPodLog, GetClusterPodLogSearch,
    GetClusterResources, GetClusterTop, InspectHelmRelease,
    ListClusterPortForwards, ListHelmReleases, PollClusterStatus,
    PollClusterStatusResponse, RollbackHelmRelease,
    RolloutClusterWorkload, ScaleClusterResource,
    SetClusterNodeSchedulable, UninstallHelmRelease,
  },
  git::{CloneRepo, PullOrCloneRepo},
};
use tokio::fs;
use tokio_util::sync::CancellationToken;

use komodo_client::matcher::Matcher;

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
    // helm spells the context flag `--kube-context`.
    Self::build_program("kubectl", "--context", target, args).await
  }

  /// Build `helm <args>` against the same kubeconfig/context.
  pub async fn build_helm(
    target: &ClusterTarget,
    args: &str,
  ) -> anyhow::Result<ClusterCommand> {
    Self::build_program("helm", "--kube-context", target, args).await
  }

  async fn build_program(
    program: &str,
    context_flag: &str,
    target: &ClusterTarget,
    args: &str,
  ) -> anyhow::Result<ClusterCommand> {
    let mut command = String::from(program);
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
      command
        .push_str(&format!(" {context_flag} {}", target.context));
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
pub(crate) async fn set_private(
  path: &std::path::Path,
) -> anyhow::Result<()> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    .await
    .context("Failed to restrict kubeconfig permissions")
}

/// Same for a directory, which needs the execute bit or nothing -
/// including the owner - can traverse into it. A directory left at
/// 0600 accepts the chmod and then fails every write inside it, and
/// only for a non-root Periphery, since root ignores the check.
#[cfg(unix)]
pub(crate) async fn set_private_dir(
  path: &std::path::Path,
) -> anyhow::Result<()> {
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
    .await
    .context("Failed to restrict directory permissions")
}

#[cfg(not(unix))]
pub(crate) async fn set_private_dir(
  _path: &std::path::Path,
) -> anyhow::Result<()> {
  Ok(())
}

#[cfg(not(unix))]
pub(crate) async fn set_private(
  _path: &std::path::Path,
) -> anyhow::Result<()> {
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
      // kubectl is already asked to give up at 10s above; this only
      // catches a kubectl that ignores its own --request-timeout.
      CommandOptions::default().timeout(POLL_CLUSTER_TIMEOUT),
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

/// A [Log::error] with secret values scrubbed out of the message.
///
/// Command paths get this for free from
/// [run_komodo_command_with_sanitization], but an error raised before
/// or after the command still reaches the Update log. Cluster config is
/// interpolated on Core, so a failure that echoes a path, repo or
/// branch echoes whatever secret was interpolated into it.
pub(crate) fn sanitized_error_log(
  stage: &str,
  e: anyhow::Error,
  replacers: &[(String, String)],
) -> Log {
  Log::error(
    stage,
    svi::replace_in_string(&format_serror(&e.into()), replacers),
  )
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
          res.logs.push(sanitized_error_log(
            "Write Manifests",
            e,
            &self.secret_replacers,
          ));
          return Ok(res);
        }
      };
    // A failed clone leaves logs but nothing to apply.
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    let result =
      apply(&self, &materialized, &mut res, &args.cancel).await;
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

/// Ceiling on a single `kubectl apply` / `delete` / `diff`.
///
/// Not a budget for how long a deploy may take — it is the point past
/// which kubectl is assumed wedged (an api server that accepted the
/// connection and then stopped answering). Without it the process-group
/// kill in [command] never fires and the execution hangs forever.
/// `delete` is the one that legitimately waits, on finalizers.
///
/// ponytail: one constant for all three modes. If a Cluster genuinely
/// needs longer, this becomes a ClusterConfig field rather than a
/// bigger number.
const KUBECTL_APPLY_TIMEOUT: Duration = Duration::from_secs(600);

/// Ceiling on one `kubectl rollout status`, which is already asked to
/// give up at 120s. This only catches a kubectl that ignores its own
/// `--timeout`, so it sits just above it.
const ROLLOUT_STATUS_TIMEOUT: Duration = Duration::from_secs(150);

/// Ceiling on the `kubectl get` that resolves what was just applied.
/// A plain read, so it gets far less rope than an apply - but it still
/// needs a bound, or a hung api server wedges the wait_ready path.
const KUBECTL_GET_TIMEOUT: Duration = Duration::from_secs(60);

/// Echoed by the diff wrapper when `kubectl diff` exits 1 (differences
/// found), so the verdict survives being mapped to success.
const DIFF_CHANGES_MARKER: &str = "__KOMODO_CLUSTER_DIFF_CHANGES__";

/// Ceiling on the `kubectl version` reachability probe.
const POLL_CLUSTER_TIMEOUT: Duration = Duration::from_secs(30);

/// Ceiling on one helm invocation. Rollback and uninstall wait on the
/// same api server an apply does, so they get the same rope.
const HELM_TIMEOUT: Duration = Duration::from_secs(600);

/// helm's OWN limit, passed as `--timeout`, deliberately below
/// [HELM_TIMEOUT].
///
/// helm keeps release state in a Secret and writes the terminal status
/// last. Killing it mid-operation - which is all HELM_TIMEOUT can do -
/// leaves the release in `pending-rollback` or `uninstalling`, and helm
/// then refuses every later operation on it with "another operation is
/// in progress". There is no force-unlock equivalent; clearing it is
/// manual. Reaching helm's own limit first lets it unwind and say which
/// hook it was waiting on. HELM_TIMEOUT stays as the backstop for a
/// helm that ignores its own flag.
const HELM_OP_TIMEOUT: Duration =
  Duration::from_secs(HELM_TIMEOUT.as_secs() - 60);

/// `--timeout` for the helm subcommands that accept it.
///
/// Only the mutating ones do: `list`, `history` and `get values` reject
/// the flag, so this is appended per command rather than centrally in
/// [run_helm].
fn helm_op_timeout_flag() -> String {
  format!(" --timeout {}s", HELM_OP_TIMEOUT.as_secs())
}

/// Refuse manifests the Cluster's policy forbids, judged on the
/// objects kubectl is about to send rather than on declared text.
///
/// Core checks the manifests a user typed, which is the fast answer
/// and the readable error. It cannot be the enforcing one: a repo- or
/// host-sourced Application has text Core never saw, a helm chart
/// produces objects that exist only after rendering, and a
/// kustomization can set `namespace:` itself and move objects out from
/// under an allow-list that was checked against the declared value.
///
/// `kubectl apply --dry-run=client` is used purely as the parser, for
/// any mode: it is the same code path, with the same `-f`/`-k`
/// handling, that the real command is about to take, so it cannot
/// disagree with it about what the object set is. Reimplementing that
/// - a yaml parser plus kustomize semantics - would be a second
/// opinion, and a policy that can be bypassed by disagreeing with
/// kubectl is not a policy.
async fn check_policy(
  req: &ApplyClusterManifests,
  source: &str,
) -> anyhow::Result<()> {
  // No controls configured, nothing to enforce, and no reason to pay
  // for the extra round trip.
  if req.policy == ManifestPolicy::default() {
    return Ok(());
  }

  let cluster_command = ClusterCommand::build(
    &req.target,
    &format!(
      "apply {source} --namespace {} --dry-run=client --validate=false -o json",
      req.namespace
    ),
  )
  .await?;
  let command = with_proxy(&req.target, &cluster_command.command);
  let log = run_komodo_command_with_sanitization(
    "Policy Check",
    command,
    CommandOptions::default().timeout(KUBECTL_APPLY_TIMEOUT),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await;
  cluster_command.cleanup().await;

  // Failing open here would make the policy advisory, so a check that
  // could not run at all - or ran and failed - is a refusal.
  let log = log.context(
    "Policy check produced no output, so the Cluster's policy could not be enforced",
  )?;
  if !log.success {
    return Err(anyhow!(
      "Could not read the objects kubectl would send, so the Cluster's policy could not be enforced | {}",
      log.stderr
    ));
  }

  for (kind, namespace) in policy_objects(&log.stdout)? {
    req
      .policy
      .check_object(&kind, &namespace)
      .with_context(|| "Cluster policy refused these manifests")?;
  }
  Ok(())
}

/// (kind, namespace) for every object in a `kubectl -o json` payload.
///
/// kubectl prints a bare object for a single manifest and a `List` for
/// several, so both shapes are accepted. An object with no `kind` is
/// an error rather than a skip: it cannot be judged, and a policy that
/// waves through what it cannot read is worse than no policy.
fn policy_objects(
  stdout: &str,
) -> anyhow::Result<Vec<(String, String)>> {
  let value: serde_json::Value = serde_json::from_str(stdout.trim())
    .context("kubectl did not return json")?;
  let items =
    if value.get("kind").and_then(|k| k.as_str()) == Some("List") {
      value
        .get("items")
        .and_then(|items| items.as_array())
        .cloned()
        .unwrap_or_default()
    } else {
      vec![value]
    };
  items
    .into_iter()
    .map(|item| {
      let kind = item
        .get("kind")
        .and_then(|kind| kind.as_str())
        .unwrap_or_default()
        .to_string();
      if kind.is_empty() {
        return Err(anyhow!(
          "kubectl returned an object with no kind, which cannot be checked against the Cluster's policy"
        ));
      }
      let namespace = item
        .get("metadata")
        .and_then(|meta| meta.get("namespace"))
        .and_then(|ns| ns.as_str())
        .unwrap_or_default()
        .to_string();
      Ok((kind, namespace))
    })
    .collect()
}

async fn apply(
  req: &ApplyClusterManifests,
  materialized: &Materialized,
  res: &mut ApplyClusterManifestsResponse,
  // The rollout wait below blocks for as long as the workload takes
  // to become ready, which for a crashlooping image is the entire
  // timeout. That is the case a user actually wants to abandon.
  cancel: &CancellationToken,
) -> anyhow::Result<()> {
  let verb = match req.mode {
    ClusterApplyMode::Apply => "apply",
    ClusterApplyMode::Delete => "delete",
    ClusterApplyMode::Diff => "diff",
  };

  // helm renders first, and what gets applied is the rendered file.
  // Rendered to disk rather than piped into kubectl on purpose: the
  // rollout wait resolves what was applied with `kubectl get -f
  // <source>`, and `-f -` has no stdin to read a second time.
  let rendered = if req.helm.is_none() {
    None
  } else {
    match render_helm(req, materialized).await {
      Ok((path, log)) => {
        res.logs.push(log);
        Some(path)
      }
      Err(e) => {
        res.logs.push(sanitized_error_log(
          "Helm Template",
          e,
          &req.secret_replacers,
        ));
        return Ok(());
      }
    }
  };

  // kustomize takes the directory; otherwise each path is a -f, and an
  // empty list means the whole directory.
  let source = if let Some(rendered) = &rendered {
    format!("-f {}", rendered.display())
  } else if req.kustomize {
    format!("-k {}", materialized.directory.display())
  } else {
    match resolve_manifest_paths(
      materialized,
      &req.exclude_file_paths,
    )
    .await
    {
      Ok(Some(paths)) => paths
        .iter()
        .map(|path| format!("-f {}", path.display()))
        .collect::<Vec<_>>()
        .join(" "),
      Ok(None) if materialized.file_paths.is_empty() => {
        format!("-f {}", materialized.directory.display())
      }
      Ok(None) => materialized
        .file_paths
        .iter()
        .map(|path| {
          format!(
            "-f {}",
            materialized.directory.join(path).display()
          )
        })
        .collect::<Vec<_>>()
        .join(" "),
      Err(e) => {
        res.logs.push(sanitized_error_log(
          "Resolve Manifests",
          e,
          &req.secret_replacers,
        ));
        return Ok(());
      }
    }
  };

  // Last gate before anything reaches the cluster, and the only one
  // that sees what is actually being sent: helm has rendered by now,
  // and kustomize is resolved by the check itself.
  if let Err(e) = check_policy(req, &source).await {
    res.logs.push(sanitized_error_log(
      "Policy Check",
      e,
      &req.secret_replacers,
    ));
    return Ok(());
  }

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
  // successful diff, so only a code above 1 is a real failure. The
  // wrapper leaves a marker behind, so that distinction survives being
  // mapped to success - otherwise Core sees "the diff worked" and has
  // no way to learn what it found.
  let command = if req.mode == ClusterApplyMode::Diff {
    format!(
      "{command}; code=$?; if [ $code -eq 1 ]; then echo {DIFF_CHANGES_MARKER}; exit 0; fi; exit $code"
    )
  } else {
    command
  };

  let stage = match req.mode {
    ClusterApplyMode::Apply => "Deploy",
    ClusterApplyMode::Delete => "Destroy",
    ClusterApplyMode::Diff => "Diff",
  };
  let mut log = run_komodo_command_with_sanitization(
    stage,
    command,
    CommandOptions::default()
      .timeout(KUBECTL_APPLY_TIMEOUT)
      .cancel(cancel.clone()),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await;
  cluster_command.cleanup().await;

  if req.mode == ClusterApplyMode::Diff
    && let Some(entry) = &mut log
    && entry.success
  {
    if entry.stdout.contains(DIFF_CHANGES_MARKER) {
      entry.stdout = entry.stdout.replace(DIFF_CHANGES_MARKER, "");
      res.changes = Some(true);
    } else {
      res.changes = Some(false);
    }
  }

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
        res.logs.push(sanitized_error_log(
          "Wait For Rollout",
          e.context("Failed to resolve applied workloads"),
          &req.secret_replacers,
        ));
        cleanup_rendered(rendered).await;
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
        CommandOptions::default()
          .timeout(ROLLOUT_STATUS_TIMEOUT)
          .cancel(cancel.clone()),
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

  cleanup_rendered(rendered).await;

  Ok(())
}

async fn cleanup_rendered(rendered: Option<PathBuf>) {
  if let Some(path) = rendered {
    let _ = fs::remove_file(path).await;
  }
}

/// `helm template` the chart into a file, and return the file plus the
/// log of the render.
///
/// Values are passed in helm's own precedence order - files in the
/// order given, then the inline block, then `--set` - so the last one
/// to mention a key wins, which is the behaviour a chart's users
/// already expect.
async fn render_helm(
  req: &ApplyClusterManifests,
  materialized: &Materialized,
) -> anyhow::Result<(PathBuf, Log)> {
  let helm = &req.helm;
  let chart = if helm.is_remote_chart() {
    helm.chart.trim().to_string()
  } else {
    materialized
      .directory
      .join(helm.chart.trim())
      .display()
      .to_string()
  };

  let dir = periphery_config().root_directory.join("clusters");
  fs::create_dir_all(&dir)
    .await
    .with_context(|| format!("Failed to create {}", dir.display()))?;
  let rendered =
    dir.join(format!("rendered-{}.yaml", random_string(10)));

  let mut args = format!(
    "template {} {chart} --namespace {}",
    helm.release_name.trim(),
    req.namespace
  );
  if !helm.version.trim().is_empty() {
    args.push_str(&format!(" --version {}", helm.version.trim()));
  }
  for values_file in &helm.values_files {
    args.push_str(&format!(
      " -f {}",
      materialized.directory.join(values_file.trim()).display()
    ));
  }
  // The inline block becomes a file, because helm has no flag for
  // "values as a string" - and it is passed after every declared
  // file, so inline beats file.
  let mut inline_values = None;
  if !helm.values.trim().is_empty() {
    let path = dir.join(format!("values-{}.yaml", random_string(10)));
    fs::write(&path, &helm.values).await.with_context(|| {
      format!("Failed to write {}", path.display())
    })?;
    args.push_str(&format!(" -f {}", path.display()));
    inline_values = Some(path);
  }
  for set in &helm.set {
    args.push_str(&format!(" --set {}", set.trim()));
  }
  for extra in &helm.extra_args {
    args.push(' ');
    args.push_str(extra);
  }

  let cluster_command =
    ClusterCommand::build_helm(&req.target, &args).await?;
  let command = format!(
    "{} > {}",
    with_proxy(&req.target, &cluster_command.command),
    rendered.display()
  );
  let log = run_komodo_command_with_sanitization(
    "Helm Template",
    command,
    CommandOptions::default().timeout(HELM_TIMEOUT),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await;
  cluster_command.cleanup().await;
  if let Some(path) = inline_values {
    // Values can hold secrets, so the file does not outlive the
    // render that needed it.
    let _ = fs::remove_file(path).await;
  }

  let Some(log) = log else {
    anyhow::bail!("helm template produced no command to run");
  };
  if !log.success {
    let _ = fs::remove_file(&rendered).await;
    anyhow::bail!(
      "helm template failed, so nothing was applied: {}",
      log.stderr
    );
  }

  // An empty render is not an apply of nothing: kubectl would accept
  // it and report success, which reads as "deployed" for a chart that
  // produced no objects at all.
  let is_empty = fs::read_to_string(&rendered)
    .await
    .map(|contents| {
      contents.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with('#') || line == "---"
      })
    })
    .unwrap_or(false);
  if is_empty {
    let _ = fs::remove_file(&rendered).await;
    anyhow::bail!(
      "helm template rendered no objects, so there is nothing to apply. Check the chart path and values."
    );
  }

  Ok((rendered, log))
}

/// Explicit `-f` paths when globs or exclusions are in play.
///
/// `Ok(None)` means the caller should use its existing behaviour -
/// the whole directory, or the declared paths as given. Only a glob
/// or an exclusion needs this to expand anything, so the common case
/// pays nothing.
async fn resolve_manifest_paths(
  materialized: &Materialized,
  exclude: &[String],
) -> anyhow::Result<Option<Vec<PathBuf>>> {
  let has_glob = materialized
    .file_paths
    .iter()
    .any(|path| path.contains('*') || path.contains('?'));
  if exclude.is_empty() && !has_glob {
    return Ok(None);
  }

  let excluders = exclude
    .iter()
    .map(|pattern| {
      Matcher::new(pattern).with_context(|| {
        format!("invalid exclude pattern '{pattern}'")
      })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;

  // kubectl reads a directory non-recursively unless asked otherwise,
  // and only these extensions, so the listing matches what applying
  // the directory would have picked up.
  let mut listing = Vec::new();
  let mut entries = fs::read_dir(&materialized.directory)
    .await
    .with_context(|| {
      format!("Failed to read {}", materialized.directory.display())
    })?;
  while let Some(entry) = entries.next_entry().await? {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let is_manifest = path
      .extension()
      .and_then(|ext| ext.to_str())
      .is_some_and(|ext| matches!(ext, "yaml" | "yml" | "json"));
    if is_manifest {
      listing.push(path);
    }
  }
  listing.sort();

  let mut selected = Vec::new();
  if materialized.file_paths.is_empty() {
    selected = listing;
  } else {
    for declared in &materialized.file_paths {
      if declared.contains('*') || declared.contains('?') {
        let matcher = Matcher::new(declared).with_context(|| {
          format!("invalid file path pattern '{declared}'")
        })?;
        for path in &listing {
          let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
          if matcher.is_match(name) && !selected.contains(path) {
            selected.push(path.clone());
          }
        }
      } else {
        let path = materialized.directory.join(declared);
        if !selected.contains(&path) {
          selected.push(path);
        }
      }
    }
  }

  selected.retain(|path| {
    let name = path
      .file_name()
      .and_then(|name| name.to_str())
      .unwrap_or_default();
    !excluders.iter().any(|matcher| {
      matcher.is_match(name)
        || matcher.is_match(&path.display().to_string())
    })
  });

  // Applying nothing is never what was meant, and kubectl would
  // happily report success for an empty `-f` list.
  if selected.is_empty() {
    anyhow::bail!(
      "No manifests left to apply after include / exclude filtering in {}",
      materialized.directory.display()
    );
  }

  Ok(Some(selected))
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
    CommandOptions::default().timeout(KUBECTL_GET_TIMEOUT),
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
    // Core validated the selector charset (no quotes / shell
    // metacharacters); the single quotes guard the spaces and
    // parentheses the selector grammar does allow.
    if let Some(selector) = &self.label_selector {
      args.push_str(&format!(" --selector '{selector}'"));
    }
    if let Some(selector) = &self.field_selector {
      args.push_str(&format!(" --field-selector '{selector}'"));
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

    let mut response: serde_json::Value =
      serde_json::from_str(&log.stdout)
        .context("kubectl returned output that is not valid json")?;

    if let Some(items) =
      response.get_mut("items").and_then(|i| i.as_array_mut())
    {
      for item in items.iter_mut() {
        if let Some(metadata) =
          item.get_mut("metadata").and_then(|m| m.as_object_mut())
        {
          // Server-side bookkeeping, often the bulk of the payload.
          metadata.remove("managedFields");
        }
      }
      if self.summary {
        *items = items.iter().map(summarize_cluster_object).collect();
      }
      // kubectl has no server-side limit for `get`, so the cluster
      // already paid for the full read - this only spares the wire
      // and the caller.
      if let Some(limit) = self.limit {
        let limit = limit as usize;
        if items.len() > limit {
          let remaining = items.len() - limit;
          items.truncate(limit);
          response["komodo_remaining_items"] = remaining.into();
        }
      }
    }

    Ok(response)
  }
}

impl Resolve<crate::api::Args> for GetClusterDescribe {
  #[instrument("GetClusterDescribe", skip_all, fields(
    kind = self.kind,
    namespace = self.namespace,
    name = self.name,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<String> {
    let mut args = format!("describe {} {}", self.kind, self.name);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Describe Resource",
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

    Ok(log.stdout)
  }
}

impl Resolve<crate::api::Args> for ExecClusterPod {
  #[instrument("ExecClusterPod", skip_all, fields(
    pod = self.pod,
    namespace = self.namespace,
    container = self.container.as_deref().unwrap_or(""),
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    // Same switch as pod exec terminals: both run commands inside
    // somebody else's process namespace.
    if periphery_config().disable_container_terminals {
      return Err(anyhow!(
        "Container Terminals are disabled in the Periphery config"
      ));
    }
    // The command must never be parsed by the host shell - only by
    // `sh` inside the container. Base64's alphabet (A-Za-z0-9+/=) is
    // inert in shell, so the host command line carries the encoding
    // and the container decodes it back.
    let encoded =
      data_encoding::BASE64.encode(self.command.as_bytes());
    let mut args = format!("exec {}", self.pod);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    if let Some(container) = &self.container {
      args.push_str(&format!(" --container {container}"));
    }
    args.push_str(&format!(
      " -- sh -c 'echo {encoded} | base64 -d | sh'"
    ));

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Exec Pod",
      command,
      Default::default(),
    )
    .await;
    cluster_command.cleanup().await;
    Ok(log)
  }
}

/// Best-effort compact row for one Kubernetes object. Fields the kind
/// does not have are omitted rather than nulled.
fn summarize_cluster_object(
  object: &serde_json::Value,
) -> serde_json::Value {
  use serde_json::{Map, Value, json};
  let mut row = Map::new();
  if let Some(kind) = object.get("kind") {
    row.insert("kind".into(), kind.clone());
  }
  let metadata = object.get("metadata");
  for key in ["name", "namespace", "creationTimestamp", "labels"] {
    if let Some(value) = metadata.and_then(|m| m.get(key)) {
      row.insert(key.into(), value.clone());
    }
  }
  let status = object.get("status");
  if let Some(phase) = status.and_then(|s| s.get("phase")) {
    row.insert("phase".into(), phase.clone());
  }
  // Pods: ready count and restarts from containerStatuses.
  if let Some(containers) = status
    .and_then(|s| s.get("containerStatuses"))
    .and_then(|c| c.as_array())
  {
    let ready = containers
      .iter()
      .filter(|c| {
        c.get("ready").and_then(Value::as_bool) == Some(true)
      })
      .count();
    let restarts: u64 = containers
      .iter()
      .filter_map(|c| c.get("restartCount").and_then(Value::as_u64))
      .sum();
    row.insert(
      "ready".into(),
      json!(format!("{ready}/{}", containers.len())),
    );
    row.insert("restarts".into(), json!(restarts));
  }
  // Workloads: readyReplicas / desired replicas.
  if let Some(desired) =
    object.get("spec").and_then(|s| s.get("replicas"))
  {
    let ready = status
      .and_then(|s| s.get("readyReplicas"))
      .and_then(Value::as_u64)
      .unwrap_or(0);
    row.insert("ready".into(), json!(format!("{ready}/{desired}")));
  }
  // Nodes and anything else condition-based: the conditions that hold.
  if let Some(conditions) = status
    .and_then(|s| s.get("conditions"))
    .and_then(|c| c.as_array())
  {
    let held: Vec<&str> = conditions
      .iter()
      .filter(|c| {
        c.get("status").and_then(Value::as_str) == Some("True")
      })
      .filter_map(|c| c.get("type").and_then(Value::as_str))
      .collect();
    if !held.is_empty() {
      row.insert("conditions".into(), json!(held));
    }
  }
  Value::Object(row)
}

impl Resolve<crate::api::Args> for GetClusterTop {
  #[instrument("GetClusterTop", skip_all, fields(
    kind = format!("{:?}", self.kind),
    namespace = self.namespace,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Vec<ClusterMetricsEntry>> {
    let mut args = match self.kind {
      ClusterMetricsKind::Nodes => String::from("top nodes"),
      ClusterMetricsKind::Pods => String::from("top pods"),
    };
    if self.kind == ClusterMetricsKind::Pods {
      if self.all_namespaces {
        args.push_str(" --all-namespaces");
      } else if !self.namespace.is_empty() {
        args.push_str(&format!(" --namespace {}", self.namespace));
      }
    }
    args.push_str(" --no-headers");

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    let log = run_komodo_standard_command(
      "Get Metrics",
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

    // `kubectl top` has no json output, so the whitespace-aligned
    // table is split by column position:
    //   nodes:    NAME CPU CPU% MEMORY MEMORY%
    //   pods:     NAME CPU MEMORY
    //   pods -A:  NAMESPACE NAME CPU MEMORY
    let all_namespaces = self.all_namespaces;
    let entries = log
      .stdout
      .lines()
      .filter_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let entry = match (self.kind, all_namespaces, cols.as_slice())
        {
          (
            ClusterMetricsKind::Nodes,
            _,
            [name, cpu, cpu_percent, memory, memory_percent],
          ) => ClusterMetricsEntry {
            name: name.to_string(),
            namespace: String::new(),
            cpu: cpu.to_string(),
            cpu_percent: cpu_percent.to_string(),
            memory: memory.to_string(),
            memory_percent: memory_percent.to_string(),
          },
          (ClusterMetricsKind::Pods, false, [name, cpu, memory]) => {
            ClusterMetricsEntry {
              name: name.to_string(),
              namespace: self.namespace.clone(),
              cpu: cpu.to_string(),
              memory: memory.to_string(),
              ..Default::default()
            }
          }
          (
            ClusterMetricsKind::Pods,
            true,
            [namespace, name, cpu, memory],
          ) => ClusterMetricsEntry {
            name: name.to_string(),
            namespace: namespace.to_string(),
            cpu: cpu.to_string(),
            memory: memory.to_string(),
            ..Default::default()
          },
          _ => return None,
        };
        Some(entry)
      })
      .collect();

    Ok(entries)
  }
}

/// Run a one-shot helm command, returning its Log.
async fn run_helm(
  target: &ClusterTarget,
  args: &str,
  stage: &str,
  secret_replacers: &[(String, String)],
  cancel: &CancellationToken,
) -> Log {
  let cluster_command =
    match ClusterCommand::build_helm(target, args).await {
      Ok(command) => command,
      Err(e) => {
        return Log::error(
          stage,
          svi::replace_in_string(
            &format_serror(&e.into()),
            secret_replacers,
          ),
        );
      }
    };
  let command = with_proxy(target, &cluster_command.command);
  let Some(log) = run_komodo_command_with_sanitization(
    stage,
    command,
    CommandOptions::default()
      .timeout(HELM_TIMEOUT)
      .cancel(cancel.clone()),
    KomodoCommandMode::Standard,
    secret_replacers,
  )
  .await
  else {
    // Only returned for an empty command, which build_helm never
    // produces.
    unreachable!()
  };
  cluster_command.cleanup().await;
  log
}

/// Run a helm command that outputs json, parsing stdout.
async fn run_helm_json(
  target: &ClusterTarget,
  args: &str,
  stage: &str,
  secret_replacers: &[(String, String)],
  cancel: &CancellationToken,
) -> anyhow::Result<serde_json::Value> {
  let log =
    run_helm(target, args, stage, secret_replacers, cancel).await;
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
  if log.stdout.trim().is_empty() {
    // `helm get values` prints "null" for releases installed with
    // defaults, but guard empty output too.
    return Ok(serde_json::Value::Null);
  }
  serde_json::from_str(&log.stdout)
    .context("helm returned output that is not valid json")
}

impl Resolve<crate::api::Args> for ListHelmReleases {
  #[instrument("ListHelmReleases", skip_all, fields(
    namespace = self.namespace,
  ))]
  async fn resolve(
    self,
    api_args: &crate::api::Args,
  ) -> anyhow::Result<serde_json::Value> {
    let mut args = String::from("list --output json");
    if self.all_namespaces {
      args.push_str(" --all-namespaces");
    } else if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    run_helm_json(
      &self.target,
      &args,
      "List Releases",
      &self.secret_replacers,
      &api_args.cancel,
    )
    .await
  }
}

impl Resolve<crate::api::Args> for InspectHelmRelease {
  #[instrument("InspectHelmRelease", skip_all, fields(
    name = self.name,
    namespace = self.namespace,
  ))]
  async fn resolve(
    self,
    api_args: &crate::api::Args,
  ) -> anyhow::Result<serde_json::Value> {
    let namespace = if self.namespace.is_empty() {
      String::new()
    } else {
      format!(" --namespace {}", self.namespace)
    };
    let history = run_helm_json(
      &self.target,
      &format!("history {}{namespace} --output json", self.name),
      "Release History",
      &self.secret_replacers,
      &api_args.cancel,
    )
    .await?;
    let values = run_helm_json(
      &self.target,
      &format!("get values {}{namespace} --output json", self.name),
      "Release Values",
      &self.secret_replacers,
      &api_args.cancel,
    )
    .await?;
    Ok(serde_json::json!({ "history": history, "values": values }))
  }
}

impl Resolve<crate::api::Args> for RollbackHelmRelease {
  #[instrument("RollbackHelmRelease", skip_all, fields(
    name = self.name,
    namespace = self.namespace,
    revision = self.revision,
  ))]
  async fn resolve(
    self,
    api_args: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!("rollback {}", self.name);
    if let Some(revision) = self.revision {
      args.push_str(&format!(" {revision}"));
    }
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    args.push_str(&helm_op_timeout_flag());
    Ok(
      run_helm(
        &self.target,
        &args,
        "Rollback Release",
        &self.secret_replacers,
        &api_args.cancel,
      )
      .await,
    )
  }
}

impl Resolve<crate::api::Args> for UninstallHelmRelease {
  #[instrument("UninstallHelmRelease", skip_all, fields(
    name = self.name,
    namespace = self.namespace,
  ))]
  async fn resolve(
    self,
    api_args: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut args = format!("uninstall {}", self.name);
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    args.push_str(&helm_op_timeout_flag());
    Ok(
      run_helm(
        &self.target,
        &args,
        "Uninstall Release",
        &self.secret_replacers,
        &api_args.cancel,
      )
      .await,
    )
  }
}

impl Resolve<crate::api::Args> for CreateClusterPortForward {
  #[instrument("CreateClusterPortForward", skip_all, fields(
    session = self.session,
    resource = self.resource,
    namespace = self.namespace,
    local_port = self.local_port,
    remote_port = self.remote_port,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<ClusterPortForward> {
    // The command execs kubectl directly (no shell), so these
    // checks guard kubectl's own argument parsing, not injection.
    if !self
      .resource
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || "./-_".contains(c))
    {
      return Err(anyhow!(
        "Invalid resource '{}': expected pod/<name> or service/<name>",
        self.resource
      ));
    }
    let address = if self.address.is_empty() {
      "127.0.0.1".to_string()
    } else {
      self.address
    };
    if !address
      .chars()
      .all(|c| c.is_ascii_alphanumeric() || ".:".contains(c))
    {
      return Err(anyhow!("Invalid address '{address}'"));
    }

    let mut forwards = crate::state::port_forwards().lock().await;
    if let Some(existing) = forwards.get_mut(&self.session) {
      // A dead session with the same name is replaced, a live one
      // is an error.
      if existing.child.try_wait()?.is_none() {
        return Err(anyhow!(
          "Port forward session '{}' already exists",
          self.session
        ));
      }
      forwards.remove(&self.session);
    }

    // A managed kubeconfig must outlive this call, like the pod
    // exec terminals: it is cleaned up with the root directory.
    let prefix =
      ClusterCommand::build_persistent(&self.target, "").await?;
    let mut tokens = prefix.split_whitespace();
    let program = tokens
      .next()
      .context("Empty kubectl command, this is a bug")?;
    let mut command = tokio::process::Command::new(program);
    command.args(tokens);
    command.args([
      "port-forward",
      &self.resource,
      &format!("{}:{}", self.local_port, self.remote_port),
      "--address",
      &address,
    ]);
    if !self.namespace.is_empty() {
      command.args(["--namespace", &self.namespace]);
    }
    if !self.target.proxy_url.is_empty() {
      command.env("HTTPS_PROXY", &self.target.proxy_url);
    }
    command
      .stdin(std::process::Stdio::null())
      .stdout(std::process::Stdio::null())
      .stderr(std::process::Stdio::piped())
      .kill_on_drop(true);

    let mut child =
      command.spawn().context("Failed to spawn kubectl")?;

    // kubectl binds before printing anything, so a short grace
    // period catches immediate failures (port taken, bad resource)
    // and returns their stderr instead of a dead session.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    if child.try_wait()?.is_some() {
      let mut stderr = String::new();
      if let Some(mut pipe) = child.stderr.take() {
        use tokio::io::AsyncReadExt;
        let _ = pipe.read_to_string(&mut stderr).await;
      }
      return Err(anyhow!(
        "kubectl port-forward exited immediately: {}",
        stderr.trim()
      ));
    }

    let info = ClusterPortForward {
      name: self.session.clone(),
      resource: self.resource,
      namespace: self.namespace,
      local_port: self.local_port,
      remote_port: self.remote_port,
      address,
      alive: true,
    };
    forwards.insert(
      self.session,
      crate::state::PortForwardSession {
        child,
        info: info.clone(),
      },
    );
    Ok(info)
  }
}

impl Resolve<crate::api::Args> for ListClusterPortForwards {
  #[instrument("ListClusterPortForwards", skip_all)]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Vec<ClusterPortForward>> {
    let mut forwards = crate::state::port_forwards().lock().await;
    let mut out = Vec::new();
    for (session, forward) in forwards.iter_mut() {
      if !session.starts_with(&self.prefix) {
        continue;
      }
      let mut info = forward.info.clone();
      info.alive = forward.child.try_wait()?.is_none();
      out.push(info);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
  }
}

impl Resolve<crate::api::Args> for DeleteClusterPortForward {
  #[instrument("DeleteClusterPortForward", skip_all, fields(
    session = self.session,
  ))]
  async fn resolve(
    self,
    _: &crate::api::Args,
  ) -> anyhow::Result<Log> {
    let mut forwards = crate::state::port_forwards().lock().await;
    let Some(mut forward) = forwards.remove(&self.session) else {
      return Err(anyhow!(
        "No port forward session '{}'",
        self.session
      ));
    };
    let _ = forward.child.kill().await;
    Ok(Log::simple(
      "Delete Port Forward",
      format!("Stopped port forward session '{}'", self.session),
    ))
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

    let mut args = match self.mode {
      ClusterObjectMode::Apply => {
        format!("apply -f {}", path.display())
      }
      ClusterObjectMode::DryRun => {
        format!("apply --dry-run=server -f {}", path.display())
      }
      ClusterObjectMode::Diff => {
        format!("diff -f {}", path.display())
      }
    };
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }

    let cluster_command =
      ClusterCommand::build(&self.target, &args).await?;
    let command = with_proxy(&self.target, &cluster_command.command);
    // `kubectl diff` exits 1 to mean "differences found", which is a
    // successful diff, so only a code above 1 is a real failure.
    let command = if self.mode == ClusterObjectMode::Diff {
      format!(
        "{command}; code=$?; if [ $code -eq 1 ]; then echo {DIFF_CHANGES_MARKER}; exit 0; fi; exit $code"
      )
    } else {
      command
    };
    let stage = match self.mode {
      ClusterObjectMode::Apply => "Apply Object",
      ClusterObjectMode::DryRun => "Dry Run Object",
      ClusterObjectMode::Diff => "Diff Object",
    };
    let mut log =
      run_komodo_standard_command(stage, command, Default::default())
        .await;
    cluster_command.cleanup().await;
    let _ = fs::remove_file(&path).await;

    if self.mode == ClusterObjectMode::Diff && log.success {
      if log.stdout.contains(DIFF_CHANGES_MARKER) {
        log.stdout = log
          .stdout
          .replace(DIFF_CHANGES_MARKER, "")
          .trim_end()
          .to_string();
      } else {
        log.stdout.push_str("No changes.");
      }
    }

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
    let mut args = String::from("logs");
    match (&self.pod, &self.label_selector) {
      (Some(pod), _) => args.push_str(&format!(" {pod}")),
      (None, Some(selector)) => {
        // Core validated the selector charset; the quotes guard the
        // spaces and parentheses the selector grammar allows.
        args.push_str(&format!(" --selector '{selector}' --prefix"));
      }
      (None, None) => {
        return Err(anyhow!(
          "One of pod / label_selector must be set"
        ));
      }
    }
    if !self.namespace.is_empty() {
      args.push_str(&format!(" --namespace {}", self.namespace));
    }
    if self.all_containers {
      args.push_str(" --all-containers");
    } else if let Some(container) = &self.container {
      args.push_str(&format!(" --container {container}"));
    }
    if let Some(since) = &self.since {
      args.push_str(&format!(" --since {since}"));
    }
    if let Some(since_time) = &self.since_time {
      args.push_str(&format!(" --since-time {since_time}"));
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
#[cfg(test)]
mod tests {
  use super::*;

  /// The summary row must carry the fields an agent triages on, per
  /// kind shape: pod readiness/restarts, workload replica counts,
  /// node conditions - and omit what the object does not have.
  #[test]
  fn summarizes_pod_workload_and_node_shapes() {
    let pod: serde_json::Value = serde_json::from_str(
      r#"{
        "kind": "Pod",
        "metadata": {
          "name": "api-1", "namespace": "app",
          "labels": { "app": "api" },
          "creationTimestamp": "2026-08-30T00:00:00Z",
          "managedFields": [{ "manager": "kubelet" }]
        },
        "status": {
          "phase": "Running",
          "containerStatuses": [
            { "ready": true, "restartCount": 2 },
            { "ready": false, "restartCount": 0 }
          ]
        }
      }"#,
    )
    .unwrap();
    let row = summarize_cluster_object(&pod);
    assert_eq!(row["name"], "api-1");
    assert_eq!(row["phase"], "Running");
    assert_eq!(row["ready"], "1/2");
    assert_eq!(row["restarts"], 2);
    assert!(row.get("managedFields").is_none());
    assert!(row.get("conditions").is_none());

    let deployment: serde_json::Value = serde_json::from_str(
      r#"{
        "kind": "Deployment",
        "metadata": { "name": "api", "namespace": "app" },
        "spec": { "replicas": 3 },
        "status": { "readyReplicas": 1 }
      }"#,
    )
    .unwrap();
    let row = summarize_cluster_object(&deployment);
    assert_eq!(row["ready"], "1/3");

    let node: serde_json::Value = serde_json::from_str(
      r#"{
        "kind": "Node",
        "metadata": { "name": "worker-1" },
        "status": {
          "conditions": [
            { "type": "Ready", "status": "True" },
            { "type": "MemoryPressure", "status": "False" }
          ]
        }
      }"#,
    )
    .unwrap();
    let row = summarize_cluster_object(&node);
    assert_eq!(row["conditions"], serde_json::json!(["Ready"]));
  }

  /// kubectl prints a bare object for one manifest and a List for
  /// several. Reading only the List shape would silently check nothing
  /// for the single-object case, which is the common one.
  #[test]
  fn reads_both_kubectl_json_shapes() {
    let single = r#"{
      "kind": "Deployment",
      "metadata": { "name": "api", "namespace": "app" }
    }"#;
    assert_eq!(
      policy_objects(single).unwrap(),
      vec![("Deployment".to_string(), "app".to_string())]
    );

    let list = r#"{
      "kind": "List",
      "items": [
        { "kind": "Deployment", "metadata": { "namespace": "app" } },
        { "kind": "ClusterRole", "metadata": { "name": "reader" } }
      ]
    }"#;
    assert_eq!(
      policy_objects(list).unwrap(),
      vec![
        ("Deployment".to_string(), "app".to_string()),
        ("ClusterRole".to_string(), String::new()),
      ]
    );
  }

  /// A policy that waves through what it cannot read is worse than no
  /// policy, so an unreadable payload is an error, not an empty pass.
  #[test]
  fn unreadable_objects_are_refused_not_skipped() {
    assert!(policy_objects("not json at all").is_err());
    assert!(
      policy_objects(r#"{ "metadata": { "name": "nameless" } }"#)
        .is_err()
    );
    assert!(
      policy_objects(
        r#"{ "kind": "List", "items": [{ "metadata": {} }] }"#
      )
      .is_err()
    );
  }

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

  #[test]
  fn error_logs_scrub_secrets() {
    let replacers =
      vec![("hunter2".to_string(), "[[PASSWORD]]".to_string())];
    let log = sanitized_error_log(
      "Write Manifests",
      anyhow!("Failed to clone https://git:hunter2@example.com/x"),
      &replacers,
    );
    assert!(!log.stderr.contains("hunter2"), "{}", log.stderr);
    assert!(log.stderr.contains("[[PASSWORD]]"), "{}", log.stderr);
    // Context chains are formatted before scrubbing, so a secret in an
    // outer frame is caught too.
    let log = sanitized_error_log(
      "Write Manifests",
      anyhow!("inner").context("outer hunter2"),
      &replacers,
    );
    assert!(!log.stderr.contains("hunter2"), "{}", log.stderr);
  }

  /// The whole point of passing helm `--timeout` is that helm reaches
  /// its own limit BEFORE the process group is killed, so it can write
  /// a terminal release status instead of being cut off mid-write and
  /// leaving the release stuck in pending-rollback / uninstalling.
  /// If these two ever cross, the fix silently stops working - the
  /// symptom is a wedged release, which looks nothing like a timeout
  /// misconfiguration.
  #[test]
  fn helm_gets_to_time_out_before_it_is_killed() {
    assert!(
      HELM_OP_TIMEOUT < HELM_TIMEOUT,
      "helm --timeout ({HELM_OP_TIMEOUT:?}) must be under the kill \
       ceiling ({HELM_TIMEOUT:?}), or helm is SIGKILLed mid-operation \
       and leaves the release wedged",
    );
  }

  /// helm wants a duration, not a bare number - `--timeout 540` is
  /// rejected, `--timeout 540s` is not.
  #[test]
  fn the_helm_timeout_flag_carries_a_unit() {
    let flag = helm_op_timeout_flag();
    assert_eq!(
      flag,
      format!(" --timeout {}s", HELM_OP_TIMEOUT.as_secs())
    );
    assert!(
      flag.trim().ends_with('s'),
      "helm rejects a unitless duration: {flag}"
    );
  }

  /// Builds a directory of files and returns it. Uses the process id
  /// and a counter so two runs never share a path.
  async fn manifest_dir(
    name: &str,
    files: &[&str],
  ) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
      .join(format!("komodo-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir).await;
    fs::create_dir_all(&dir).await.unwrap();
    for file in files {
      fs::write(dir.join(file), "kind: ConfigMap\n")
        .await
        .unwrap();
    }
    dir
  }

  #[tokio::test]
  async fn no_globs_or_excludes_changes_nothing() {
    let dir = manifest_dir("plain", &["a.yaml"]).await;
    let materialized = Materialized {
      directory: dir.clone(),
      file_paths: vec![],
      temporary: false,
    };
    // None means "use the existing behaviour", which is what the
    // common case must keep doing.
    assert!(
      resolve_manifest_paths(&materialized, &[])
        .await
        .unwrap()
        .is_none()
    );
    let _ = fs::remove_dir_all(dir).await;
  }

  #[tokio::test]
  async fn excludes_drop_files_from_a_whole_directory() {
    let dir = manifest_dir(
      "exclude",
      &["deploy.yaml", "values.yaml", "readme.md"],
    )
    .await;
    let materialized = Materialized {
      directory: dir.clone(),
      file_paths: vec![],
      temporary: false,
    };
    let paths = resolve_manifest_paths(
      &materialized,
      &[String::from("values*.yaml")],
    )
    .await
    .unwrap()
    .unwrap();
    let names = paths
      .iter()
      .map(|path| path.file_name().unwrap().to_str().unwrap())
      .collect::<Vec<_>>();
    // readme.md was never a candidate: kubectl would not have read it.
    assert_eq!(names, vec!["deploy.yaml"]);
    let _ = fs::remove_dir_all(dir).await;
  }

  #[tokio::test]
  async fn a_glob_in_file_paths_expands() {
    let dir =
      manifest_dir("glob", &["one.yaml", "two.yaml", "other.json"])
        .await;
    let materialized = Materialized {
      directory: dir.clone(),
      file_paths: vec![String::from("*.yaml")],
      temporary: false,
    };
    let paths = resolve_manifest_paths(&materialized, &[])
      .await
      .unwrap()
      .unwrap();
    let mut names = paths
      .iter()
      .map(|path| path.file_name().unwrap().to_str().unwrap())
      .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["one.yaml", "two.yaml"]);
    let _ = fs::remove_dir_all(dir).await;
  }

  #[tokio::test]
  async fn filtering_everything_out_is_an_error() {
    let dir = manifest_dir("empty", &["values.yaml"]).await;
    let materialized = Materialized {
      directory: dir.clone(),
      file_paths: vec![],
      temporary: false,
    };
    // Applying nothing must not read as a successful apply.
    assert!(
      resolve_manifest_paths(
        &materialized,
        &[String::from("values.yaml")]
      )
      .await
      .is_err()
    );
    let _ = fs::remove_dir_all(dir).await;
  }
}
