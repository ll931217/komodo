use std::{path::PathBuf, time::Duration};

use anyhow::{Context, anyhow};
use command::{
  CommandOptions, KomodoCommandMode,
  run_komodo_command_with_sanitization,
};
use komodo_client::entities::{
  EnvironmentVar, all_logs_success, random_string,
  to_path_compatible_name, update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  git::{CloneRepo, PullOrCloneRepo},
  terraform::{
    RunTerraform, RunTerraformResponse, TerraformMode,
    TerraformSource,
  },
};
use tokio::fs;
use tokio_util::sync::CancellationToken;

use crate::{
  api::cluster::{sanitized_error_log, set_private, set_private_dir},
  config::periphery_config,
};

/// Ceiling on `terraform init`. Providers come from the image-baked
/// filesystem mirror (2s measured in Phase 0), so anything near this
/// is a unit dialing a blocked endpoint through a mis-set mirror.
const TERRAFORM_INIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Ceiling on one plan / apply / destroy. Not a budget for how long an
/// apply may take - it is the point past which terraform is assumed
/// wedged (a provider dialing a firewall-blocked endpoint hangs
/// forever, and the process-group kill in [command] only fires when a
/// timeout is set).
///
/// ponytail: one constant for all three verbs. If a unit genuinely
/// needs longer, this becomes a TerraformConfig field rather than a
/// bigger number.
const TERRAFORM_RUN_TIMEOUT: Duration = Duration::from_secs(600);

/// Ceiling on stdout / stderr stored per Log. A large plan can be
/// megabytes, and the Update log is not the place for it - the TAIL is
/// kept because terraform puts the verdict ("Plan: 2 to add...")
/// at the end.
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Marker echoed by the plan wrapper when `-detailed-exitcode`
/// returns 2 (changes present), so the exit code survives being
/// mapped to success.
const CHANGES_MARKER: &str = "__KOMODO_TERRAFORM_CHANGES__";

impl Resolve<crate::api::Args> for RunTerraform {
  #[instrument("RunTerraform", skip_all, fields(
    name = self.name,
    mode = format!("{:?}", self.mode),
  ))]
  async fn resolve(
    self,
    args: &crate::api::Args,
  ) -> anyhow::Result<RunTerraformResponse> {
    let mut res = RunTerraformResponse::default();

    let root = match materialize(&self, &mut res, args).await {
      Ok(root) => root,
      Err(e) => {
        res.logs.push(sanitized_error_log(
          "Materialize Tree",
          e,
          &self.secret_replacers,
        ));
        return Ok(res);
      }
    };
    // A failed clone leaves logs but nothing to run.
    if !all_logs_success(&res.logs) {
      return Ok(res);
    }

    let invocation = match Invocation::build(&self, &root).await {
      Ok(invocation) => invocation,
      Err(e) => {
        res.logs.push(sanitized_error_log(
          "Prepare Run",
          e,
          &self.secret_replacers,
        ));
        return Ok(res);
      }
    };

    let result =
      run(&self, &invocation, &mut res, &args.cancel).await;
    invocation.cleanup().await;
    result?;

    Ok(res)
  }
}

/// Root of the periphery-managed terraform area:
/// working dirs, managed state, and per-run temp files.
fn terraform_dir() -> PathBuf {
  periphery_config().root_directory.join("terraform")
}

/// Get the terraform tree onto disk and return its root.
///
/// Working directories are persistent and stable-named - never
/// random, never temporary, never removed: a directory that holds a
/// `.terraform/` (provider cache, backend pointer) is cheap to keep
/// and expensive to lose, and one that holds unmanaged local state
/// orphans real infrastructure if deleted.
async fn materialize(
  req: &RunTerraform,
  res: &mut RunTerraformResponse,
  args: &crate::api::Args,
) -> anyhow::Result<PathBuf> {
  match &req.source {
    TerraformSource::Contents(contents) => {
      let dir =
        terraform_dir().join(to_path_compatible_name(&req.name));
      fs::create_dir_all(&dir).await.with_context(|| {
        format!("Failed to create {}", dir.display())
      })?;
      // Contents may embed interpolated secrets; keep the whole
      // working dir private rather than chasing per-file modes.
      set_private_dir(&dir).await?;
      let path = dir.join("main.tf");
      fs::write(&path, contents).await.with_context(|| {
        format!("Failed to write {}", path.display())
      })?;
      Ok(dir)
    }

    TerraformSource::FilesOnHost { root_directory } => {
      let directory = PathBuf::from(root_directory)
        .components()
        .collect::<PathBuf>();
      if !directory.is_dir() {
        return Err(anyhow!(
          "Terraform directory {} does not exist on this host",
          directory.display()
        ));
      }
      Ok(directory)
    }

    TerraformSource::Repo {
      args: repo_args,
      git_token,
      reclone,
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

      Ok(root)
    }
  }
}

/// Everything a run needs beyond the tree: the unit directory, the
/// env file to source, and any temp files to remove afterwards so
/// interpolated credentials don't linger on disk.
struct Invocation {
  /// Absolute directory of the unit (`-chdir`).
  unit: PathBuf,
  /// Env file sourced ahead of every terraform command.
  env_file: PathBuf,
  /// `-backend-config=path=` for init, when state is managed.
  state_path: Option<PathBuf>,
  temp_files: Vec<PathBuf>,
}

impl Invocation {
  async fn build(
    req: &RunTerraform,
    root: &std::path::Path,
  ) -> anyhow::Result<Invocation> {
    let unit = root
      .join(&req.run_directory)
      .components()
      .collect::<PathBuf>();
    if !unit.is_dir() {
      return Err(anyhow!(
        "Run directory {} does not exist within the terraform tree",
        unit.display()
      ));
    }

    let tmp = terraform_dir().join(".tmp");
    fs::create_dir_all(&tmp).await.with_context(|| {
      format!("Failed to create {}", tmp.display())
    })?;
    set_private_dir(&tmp).await?;

    let mut temp_files = Vec::new();

    // Kubeconfig bridge: contents to a private temp file, or an
    // existing host path as-is.
    let kubeconfig = if !req.kubeconfig_contents.is_empty() {
      let path =
        tmp.join(format!("kubeconfig-{}", random_string(10)));
      fs::write(&path, &req.kubeconfig_contents)
        .await
        .with_context(|| {
          format!("Failed to write kubeconfig to {}", path.display())
        })?;
      set_private(&path).await?;
      let display = path.display().to_string();
      temp_files.push(path);
      Some(display)
    } else if !req.kubeconfig_path.is_empty() {
      Some(req.kubeconfig_path.clone())
    } else {
      None
    };

    // The env file carries everything that must not appear on the
    // command line (visible in the process table) - fixed automation
    // env, proxy, kubeconfig exports, then the unit's own entries.
    let mut env = String::from("TF_IN_AUTOMATION=true\n");
    if !req.proxy_url.is_empty() {
      let proxy = shell_quote(&req.proxy_url);
      env.push_str(&format!(
        "HTTP_PROXY={proxy}\nHTTPS_PROXY={proxy}\nhttp_proxy={proxy}\nhttps_proxy={proxy}\n"
      ));
      if !req.no_proxy.is_empty() {
        let no_proxy = shell_quote(&req.no_proxy);
        env.push_str(&format!(
          "NO_PROXY={no_proxy}\nno_proxy={no_proxy}\n"
        ));
      }
    }
    if let Some(kubeconfig) = kubeconfig {
      let kubeconfig = shell_quote(&kubeconfig);
      env.push_str(&format!(
        "TF_VAR_kubeconfig_path={kubeconfig}\nKUBE_CONFIG_PATH={kubeconfig}\n"
      ));
    }
    for EnvironmentVar { variable, value } in &req.environment {
      env.push_str(&format!("{variable}={}\n", shell_quote(value)));
    }

    let env_file = tmp.join(format!("env-{}", random_string(10)));
    fs::write(&env_file, env).await.with_context(|| {
      format!("Failed to write env file to {}", env_file.display())
    })?;
    set_private(&env_file).await?;
    temp_files.push(env_file.clone());

    let state_path = if req.managed_state {
      let state_dir = terraform_dir().join("state");
      fs::create_dir_all(&state_dir).await.with_context(|| {
        format!("Failed to create {}", state_dir.display())
      })?;
      // State contains secrets in plaintext by design; terraform
      // itself creates the file world-readable, so the directory is
      // what carries the restriction.
      set_private_dir(&state_dir).await?;
      Some(state_dir.join(format!(
        "{}.tfstate",
        to_path_compatible_name(&req.name)
      )))
    } else {
      None
    };

    Ok(Invocation {
      unit,
      env_file,
      state_path,
      temp_files,
    })
  }

  /// `set -a; . env; set +a; terraform -chdir=<unit> <args>`
  fn command(&self, args: &str) -> String {
    format!(
      "set -a && . {} && set +a && terraform -chdir={} {args}",
      self.env_file.display(),
      self.unit.display(),
    )
  }

  async fn cleanup(self) {
    for path in self.temp_files {
      let _ = fs::remove_file(path).await;
    }
  }
}

async fn run(
  req: &RunTerraform,
  invocation: &Invocation,
  res: &mut RunTerraformResponse,
  // Both commands below take it: an apply that cannot be interrupted
  // is the one you most want to interrupt, and init can hang for the
  // whole timeout on an unreachable state backend.
  cancel: &CancellationToken,
) -> anyhow::Result<()> {
  // Init on every run: from the local mirror it costs ~2s, and it is
  // what re-points the backend after a reclone wiped `.terraform/`.
  let mut init_args = String::from("init -input=false");
  if let Some(state_path) = &invocation.state_path {
    init_args.push_str(&format!(
      " -backend-config=path={}",
      state_path.display()
    ));
  }
  let Some(mut init_log) = run_komodo_command_with_sanitization(
    "Terraform Init",
    invocation.command(&init_args),
    CommandOptions::default()
      .timeout(TERRAFORM_INIT_TIMEOUT)
      .cancel(cancel.clone()),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await
  else {
    return Err(anyhow!("Terraform init command was empty"));
  };
  truncate_log(&mut init_log);
  let init_ok = init_log.success;
  res.logs.push(init_log);
  if !init_ok {
    return Ok(());
  }

  let extra_args =
    req.extra_args.iter().fold(String::new(), |mut acc, arg| {
      acc.push(' ');
      acc.push_str(arg);
      acc
    });

  let (stage, command) = match req.mode {
    TerraformMode::Plan => (
      "Terraform Plan",
      // -detailed-exitcode: 0 = clean, 2 = changes, 1 = error. The
      // wrapper maps 2 to success and leaves a marker so the
      // distinction survives into the response.
      format!(
        "{}; code=$?; if [ \"$code\" -eq 2 ]; then echo {CHANGES_MARKER}; exit 0; fi; exit $code",
        invocation.command(&format!(
          "plan -input=false -detailed-exitcode{extra_args}"
        ))
      ),
    ),
    TerraformMode::Apply => (
      "Terraform Apply",
      invocation.command(&format!(
        "apply -input=false -auto-approve{extra_args}"
      )),
    ),
    TerraformMode::Destroy => (
      "Terraform Destroy",
      invocation.command(&format!(
        "destroy -input=false -auto-approve{extra_args}"
      )),
    ),
  };

  let Some(mut log) = run_komodo_command_with_sanitization(
    stage,
    command,
    CommandOptions::default()
      .timeout(TERRAFORM_RUN_TIMEOUT)
      .cancel(cancel.clone()),
    KomodoCommandMode::Shell,
    &req.secret_replacers,
  )
  .await
  else {
    return Err(anyhow!("Terraform command was empty"));
  };

  if req.mode == TerraformMode::Plan && log.success {
    if log.stdout.contains(CHANGES_MARKER) {
      log.stdout = log.stdout.replace(CHANGES_MARKER, "");
      res.changes = Some(true);
    } else {
      res.changes = Some(false);
    }
  }

  truncate_log(&mut log);
  res.logs.push(log);
  Ok(())
}

/// Single-quote a value for the env file, so spaces and shell
/// metacharacters survive `. env` intact.
fn shell_quote(value: &str) -> String {
  format!("'{}'", value.replace('\'', r#"'\''"#))
}

/// Keep each stream under [MAX_OUTPUT_BYTES], dropping the HEAD:
/// terraform puts the verdict at the end.
fn truncate_log(log: &mut Log) {
  for output in [&mut log.stdout, &mut log.stderr] {
    if output.len() > MAX_OUTPUT_BYTES {
      let dropped = output.len() - MAX_OUTPUT_BYTES;
      // Cut on a char boundary at or after the byte target.
      let mut cut = dropped;
      while !output.is_char_boundary(cut) {
        cut += 1;
      }
      *output = format!(
        "[Komodo: truncated {dropped} leading bytes]\n{}",
        &output[cut..]
      );
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shell_quote_survives_sourcing() {
    assert_eq!(shell_quote("plain"), "'plain'");
    assert_eq!(
      shell_quote("it's a $VAR `cmd`"),
      r#"'it'\''s a $VAR `cmd`'"#
    );
  }

  #[test]
  fn truncate_keeps_the_tail() {
    let mut log = Log::simple("t", "x".repeat(MAX_OUTPUT_BYTES + 7));
    log.stdout.push_str("Plan: 1 to add");
    truncate_log(&mut log);
    assert!(log.stdout.len() <= MAX_OUTPUT_BYTES + 64);
    assert!(log.stdout.ends_with("Plan: 1 to add"));
    assert!(log.stdout.starts_with("[Komodo: truncated"));
  }
}
