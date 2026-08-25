use std::path::PathBuf;

use anyhow::{Context, anyhow};
use formatting::format_serror;
use komodo_client::entities::{
  FileContents, RepoExecutionArgs, all_logs_success, repo::Repo,
  stack::Stack, to_path_compatible_name, update::Log,
};
use mogh_resolver::Resolve;
use periphery_client::api::{
  DeployStackResponse,
  compose::{ComposePullResponse, ComposeRunResponse},
  git::{CloneRepo, PullOrCloneRepo},
};
use tokio::fs;

use crate::{api::Args, config::periphery_config, helpers};

pub trait WriteStackRes {
  fn logs(&mut self) -> &mut Vec<Log>;
  fn add_remote_error(&mut self, _contents: FileContents) {}
  fn set_commit_hash(&mut self, _hash: Option<String>) {}
  fn set_commit_message(&mut self, _message: Option<String>) {}
}

impl WriteStackRes for &mut DeployStackResponse {
  fn logs(&mut self) -> &mut Vec<Log> {
    &mut self.logs
  }
  fn add_remote_error(&mut self, contents: FileContents) {
    self.remote_errors.push(contents);
  }
  fn set_commit_hash(&mut self, hash: Option<String>) {
    self.commit_hash = hash;
  }
  fn set_commit_message(&mut self, message: Option<String>) {
    self.commit_message = message;
  }
}

impl WriteStackRes for &mut ComposePullResponse {
  fn logs(&mut self) -> &mut Vec<Log> {
    &mut self.logs
  }
}

impl WriteStackRes for &mut ComposeRunResponse {
  fn logs(&mut self) -> &mut Vec<Log> {
    &mut self.logs
  }
}

/// Where [write_stack] would put this Stack, without writing or
/// cloning anything.
///
/// For the destroy path, which needs a working directory for its hooks
/// but must not clone: a repo that has since been deleted, or a token
/// that has since expired, would otherwise block tearing the Stack
/// down. Derived here rather than recomputed at the call site so it
/// cannot drift from where [write_stack] actually puts things - the
/// four branches below are the same four, in the same order.
pub fn stack_run_directory(
  stack: &Stack,
  repo: Option<&Repo>,
) -> PathBuf {
  let root = if stack.config.files_on_host {
    // The only branch whose run_directory is joined onto the stack
    // dir directly, with no clone root in between.
    periphery_config()
      .stack_dir()
      .join(to_path_compatible_name(&stack.name))
  } else if let Some(repo) = repo {
    periphery_config()
      .repo_dir()
      .join(to_path_compatible_name(&repo.name))
      .join(&repo.config.path)
  } else if !stack.config.repo.is_empty() {
    periphery_config()
      .stack_dir()
      .join(to_path_compatible_name(&stack.name))
      .join(&stack.config.clone_path)
  } else {
    // UI-defined: contents are written straight into the stack dir,
    // with no run_directory below it.
    return periphery_config()
      .stack_dir()
      .join(to_path_compatible_name(&stack.name))
      .components()
      .collect();
  };
  root
    .join(&stack.config.run_directory)
    .components()
    .collect()
}

/// Either writes the stack file_contents to a file, or clones the repo.
/// Asssumes all interpolation is already complete.
/// Returns (run_directory, env_file_path, periphery_replacers)
#[instrument(
  "WriteStack",
  skip_all,
  fields(
    stack = stack.name,
    repo = repo.as_ref().map(|repo| &repo.name),
  )
)]
pub async fn write_stack<'a>(
  stack: &'a Stack,
  repo: Option<&Repo>,
  git_token: Option<String>,
  replacers: Vec<(String, String)>,
  res: impl WriteStackRes,
  req_args: &Args,
) -> anyhow::Result<(
  // run_directory
  PathBuf,
  // env_file_path
  Option<&'a str>,
)> {
  if stack.config.files_on_host {
    write_stack_files_on_host(stack, res).await
  } else if let Some(repo) = repo {
    write_stack_linked_repo(
      stack, repo, git_token, replacers, res, req_args,
    )
    .await
  } else if !stack.config.repo.is_empty() {
    write_stack_inline_repo(stack, git_token, res, req_args).await
  } else {
    write_stack_ui_defined(stack, res).await
  }
}

#[instrument("WriteStackFilesOnHost", skip_all)]
async fn write_stack_files_on_host(
  stack: &Stack,
  mut res: impl WriteStackRes,
) -> anyhow::Result<(
  // run_directory
  PathBuf,
  // env_file_path
  Option<&str>,
)> {
  let run_directory = periphery_config()
    .stack_dir()
    .join(to_path_compatible_name(&stack.name))
    .join(&stack.config.run_directory)
    .components()
    .collect::<PathBuf>();
  let env_file_path = environment::write_env_file(
    &stack.config.env_vars()?,
    run_directory.as_path(),
    &stack.config.env_file_path,
    res.logs(),
  )
  .await;
  if all_logs_success(res.logs()) {
    Ok((
      run_directory,
      // Env file paths are expected to be already relative to run directory,
      // so need to pass original env_file_path here.
      env_file_path
        .is_some()
        .then_some(&stack.config.env_file_path),
    ))
  } else {
    Err(anyhow!("Failed to write env file, stopping run."))
  }
}

#[instrument("WriteStackLinkedRepo", skip_all)]
async fn write_stack_linked_repo<'a>(
  stack: &'a Stack,
  repo: &Repo,
  git_token: Option<String>,
  replacers: Vec<(String, String)>,
  mut res: impl WriteStackRes,
  req_args: &Args,
) -> anyhow::Result<(
  // run_directory
  PathBuf,
  // env_file_path
  Option<&'a str>,
)> {
  let root = periphery_config()
    .repo_dir()
    .join(to_path_compatible_name(&repo.name))
    .join(&repo.config.path)
    .components()
    .collect::<PathBuf>();

  let mut args: RepoExecutionArgs = repo.into();
  // Set the clone destination to the one created for this run
  args.destination = Some(root.display().to_string());

  let git_token = stack_git_token(git_token, &args, &mut res)?;

  let env_file_path = root
    .join(&repo.config.env_file_path)
    .components()
    .collect::<PathBuf>()
    .display()
    .to_string();

  let on_clone = (!repo.config.on_clone.is_none())
    .then_some(repo.config.on_clone.clone());
  let on_pull = (!repo.config.on_pull.is_none())
    .then_some(repo.config.on_pull.clone());

  let clone_res = if stack.config.reclone {
    CloneRepo {
      args,
      git_token,
      environment: repo.config.env_vars()?,
      env_file_path,
      on_clone,
      on_pull,
      skip_secret_interp: repo.config.skip_secret_interp,
      replacers,
    }
    .resolve(req_args)
    .await?
  } else {
    PullOrCloneRepo {
      args,
      git_token,
      environment: repo.config.env_vars()?,
      env_file_path,
      on_clone,
      on_pull,
      skip_secret_interp: repo.config.skip_secret_interp,
      replacers,
    }
    .resolve(req_args)
    .await?
  };

  res.logs().extend(clone_res.res.logs);
  res.set_commit_hash(clone_res.res.commit_hash);
  res.set_commit_message(clone_res.res.commit_message);

  if !all_logs_success(res.logs()) {
    return Ok((root, None));
  }

  let run_directory = root
    .join(&stack.config.run_directory)
    .components()
    .collect::<PathBuf>();

  let env_file_path = environment::write_env_file(
    &stack.config.env_vars()?,
    run_directory.as_path(),
    &stack.config.env_file_path,
    res.logs(),
  )
  .await;
  if !all_logs_success(res.logs()) {
    return Err(anyhow!("Failed to write env file, stopping run"));
  }

  Ok((
    run_directory,
    env_file_path
      .is_some()
      .then_some(&stack.config.env_file_path),
  ))
}

#[instrument("WriteStackInlineRepo", skip_all)]
async fn write_stack_inline_repo<'a>(
  stack: &'a Stack,
  git_token: Option<String>,
  mut res: impl WriteStackRes,
  req_args: &Args,
) -> anyhow::Result<(
  // run_directory
  PathBuf,
  // env_file_path
  Option<&'a str>,
)> {
  let root = periphery_config()
    .stack_dir()
    .join(to_path_compatible_name(&stack.name))
    .join(&stack.config.clone_path)
    .components()
    .collect::<PathBuf>();

  let mut args: RepoExecutionArgs = stack.into();
  // Set the clone destination to the one created for this run
  args.destination = Some(root.display().to_string());

  let git_token = stack_git_token(git_token, &args, &mut res)?;

  let clone_res = if stack.config.reclone {
    CloneRepo {
      args,
      git_token,
      environment: Default::default(),
      env_file_path: Default::default(),
      on_clone: Default::default(),
      on_pull: Default::default(),
      skip_secret_interp: Default::default(),
      replacers: Default::default(),
    }
    .resolve(req_args)
    .await?
  } else {
    PullOrCloneRepo {
      args,
      git_token,
      environment: Default::default(),
      env_file_path: Default::default(),
      on_clone: Default::default(),
      on_pull: Default::default(),
      skip_secret_interp: Default::default(),
      replacers: Default::default(),
    }
    .resolve(req_args)
    .await?
  };

  res.logs().extend(clone_res.res.logs);
  res.set_commit_hash(clone_res.res.commit_hash);
  res.set_commit_message(clone_res.res.commit_message);

  if !all_logs_success(res.logs()) {
    return Ok((root, None));
  }

  let run_directory = root
    .join(&stack.config.run_directory)
    .components()
    .collect::<PathBuf>();

  let env_file_path = environment::write_env_file(
    &stack.config.env_vars()?,
    run_directory.as_path(),
    &stack.config.env_file_path,
    res.logs(),
  )
  .await;
  if !all_logs_success(res.logs()) {
    return Err(anyhow!("Failed to write env file, stopping run"));
  }

  Ok((
    run_directory,
    env_file_path
      .is_some()
      .then_some(&stack.config.env_file_path),
  ))
}

#[instrument("WriteStackUiDefined", skip_all)]
async fn write_stack_ui_defined(
  stack: &Stack,
  mut res: impl WriteStackRes,
) -> anyhow::Result<(
  // run_directory
  PathBuf,
  // env_file_path
  Option<&str>,
)> {
  if stack.config.file_contents.trim().is_empty() {
    return Err(anyhow!(
      "Must either input compose file contents directly, or use files on host / git repo options."
    ));
  }

  let run_directory = periphery_config()
    .stack_dir()
    .join(to_path_compatible_name(&stack.name))
    .components()
    .collect::<PathBuf>();

  // Ensure run directory exists
  fs::create_dir_all(&run_directory).await.with_context(|| {
    format!(
      "failed to create stack run directory at {run_directory:?}"
    )
  })?;
  let env_file_path = environment::write_env_file(
    &stack.config.env_vars()?,
    run_directory.as_path(),
    &stack.config.env_file_path,
    res.logs(),
  )
  .await;
  if !all_logs_success(res.logs()) {
    return Err(anyhow!("Failed to write env file, stopping run"));
  }

  let file_path = run_directory
    .join(
      stack
        .config
        .file_paths
        // only need the first one, or default
        .first()
        .map(String::as_str)
        .unwrap_or("compose.yaml"),
    )
    .components()
    .collect::<PathBuf>();

  mogh_secret_file::write_async(
    &file_path,
    &stack.config.file_contents,
  )
  .await
  .with_context(|| {
    format!("Failed to write compose file to {file_path:?}")
  })?;

  Ok((
    run_directory,
    env_file_path
      .is_some()
      .then_some(&stack.config.env_file_path),
  ))
}

fn stack_git_token<R: WriteStackRes>(
  core_token: Option<String>,
  args: &RepoExecutionArgs,
  res: &mut R,
) -> anyhow::Result<Option<String>> {
  helpers::git_token(core_token, args).map_err(|e| {
    let error = format_serror(&e.into());
    res
      .logs()
      .push(Log::error("Missing git token", error.clone()));
    res.add_remote_error(FileContents {
      path: Default::default(),
      contents: error,
    });
    anyhow!("failed to find required git token, stopping run")
  })
}
