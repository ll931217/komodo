use std::{io::ErrorKind, path::Path};

use anyhow::Context;
use command::{CommandOptions, run_komodo_standard_command};
use formatting::format_serror;
use komodo_client::entities::{
  RepoExecutionArgs, RepoExecutionResponse, all_logs_success,
  update::Log,
};

use crate::{check_installed, get_commit_hash_log};

/// Will delete the existing repo folder,
/// clone the repo, get the latest hash / message,
/// and run on_clone / on_pull.
///
/// Assumes all interpolation is already done and takes the list of replacers
/// for the On Clone command.
pub async fn clone<T>(
  clone_args: T,
  root_repo_dir: &Path,
  access_token: Option<String>,
) -> anyhow::Result<RepoExecutionResponse>
where
  T: Into<RepoExecutionArgs> + std::fmt::Debug,
{
  check_installed().await?;

  let args: RepoExecutionArgs = clone_args.into();
  // Tokenless: the credential travels in the environment instead, so
  // it never lands in the clone's .git/config as origin.
  let repo_url = args.remote_url(None)?;

  let mut res = RepoExecutionResponse {
    path: args.path(root_repo_dir),
    logs: Vec::new(),
    commit_hash: None,
    commit_message: None,
  };

  // Ensure parent folder exists
  if let Some(parent) = res.path.parent()
    && let Err(e) = tokio::fs::create_dir_all(parent)
      .await
      .context("Failed to create clone parent directory.")
  {
    res.logs.push(Log::error(
      "Prepare Repo Root",
      format_serror(&e.into()),
    ));
    return Ok(res);
  }

  match tokio::fs::remove_dir_all(&res.path).await {
    Err(e) if e.kind() != ErrorKind::NotFound => {
      let e: anyhow::Error = e.into();
      res.logs.push(Log::error(
        "Clean Repo Root",
        format_serror(
          &e.context(
            "Failed to remove existing repo root before clone.",
          )
          .into(),
        ),
      ));
      return Ok(res);
    }
    _ => {}
  }

  // Built before the command so the key file exists for its whole run,
  // and dropped at the end of this function so it is removed whether the
  // clone succeeded, failed or panicked.
  let ssh = match crate::ssh::session_for(&args, &args.name).await {
    Ok(session) => session,
    Err(e) => {
      res.logs.push(Log::error(
        "Prepare SSH Key",
        format_serror(&e.into()),
      ));
      return Ok(res);
    }
  };

  let command = crate::credentials::git_command(
    access_token.as_deref(),
    &format!(
      "clone {repo_url} {} -b {}",
      res.path.display(),
      args.branch
    ),
  );

  // Timed here rather than around the whole function: the surrounding
  // steps are local (dir prep, reading the latest commit) and folding
  // them in would move the number for non-network reasons.
  let started = std::time::Instant::now();
  let mut log = run_komodo_standard_command(
    "Clone Repo",
    command,
    crate::ssh::with_ssh(
      crate::credentials::with_credential(
        CommandOptions::default(),
        access_token.as_deref(),
      ),
      ssh.as_ref(),
    ),
  )
  .await;
  crate::metrics::observe(
    crate::metrics::GitOp::Clone,
    log.success,
    started.elapsed(),
  );

  // Via the helper so the empty-token case is handled in one tested
  // place: an empty pattern matches at every position, so redacting a
  // blank token would shred the log rather than protect it.
  let token = access_token.as_deref();
  log.command = crate::credentials::redact_token(&log.command, token);
  log.stdout = crate::credentials::redact_token(&log.stdout, token);
  log.stderr = crate::credentials::redact_token(&log.stderr, token);

  res.logs.push(log);

  if !all_logs_success(&res.logs) {
    return Ok(res);
  }

  if let Some(commit) = args.commit {
    let reset_log = run_komodo_standard_command(
      "set commit",
      format!("git reset --hard {commit}",),
      CommandOptions::default().path(res.path.as_path()),
    )
    .await;
    res.logs.push(reset_log);
  }

  if !all_logs_success(&res.logs) {
    return Ok(res);
  }

  match get_commit_hash_log(&res.path)
    .await
    .context("Failed to get latest commit")
  {
    Ok((log, hash, message)) => {
      res.logs.push(log);
      res.commit_hash = Some(hash);
      res.commit_message = Some(message);
    }
    Err(e) => {
      res
        .logs
        .push(Log::simple("Latest Commit", format_serror(&e.into())));
    }
  };

  Ok(res)
}
