use std::path::Path;

use command::{CommandOptions, run_komodo_standard_command};
use formatting::format_serror;
use komodo_client::entities::{
  RepoExecutionArgs, all_logs_success, update::Log,
};

use crate::check_installed;

pub async fn init_folder_as_repo(
  folder_path: &Path,
  args: &RepoExecutionArgs,
  // No credential here on purpose: this only writes a tokenless origin.
  // Anything that talks to the remote carries its own via
  // crate::credentials.
  logs: &mut Vec<Log>,
) {
  if let Err(e) = check_installed().await {
    logs.push(Log::error("Git Init", format_serror(&e.into())));
    return;
  };

  // Initialize the folder as a git repo
  let init_repo = run_komodo_standard_command(
    "Git Init",
    "git init",
    CommandOptions::default().path(folder_path),
  )
  .await;
  logs.push(init_repo);
  if !all_logs_success(logs) {
    return;
  }

  // Tokenless: origin must not carry the credential on disk.
  let repo_url = match args.remote_url(None) {
    Ok(url) => url,
    Err(e) => {
      logs
        .push(Log::error("Add git remote", format_serror(&e.into())));
      return;
    }
  };

  // Set remote url
  // No sanitizing needed: the url has no credential in it now.
  let set_remote = run_komodo_standard_command(
    "Add git remote",
    format!("git remote add origin {repo_url}"),
    CommandOptions::default().path(folder_path),
  )
  .await;
  if !set_remote.success {
    logs.push(set_remote);
    return;
  }

  // Set branch.
  let init_repo = run_komodo_standard_command(
    "Set Branch",
    format!("git switch -c {}", args.branch),
    CommandOptions::default().path(folder_path),
  )
  .await;
  if !init_repo.success {
    logs.push(init_repo);
  }
}
