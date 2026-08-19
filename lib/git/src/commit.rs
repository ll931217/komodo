use std::path::{Path, PathBuf};

use anyhow::Context;
use command::{
  CommandOptions, run_komodo_standard_command, run_standard_command,
};
use formatting::format_serror;
use komodo_client::entities::{
  RepoExecutionResponse, TlsAuth, all_logs_success, update::Log,
};

use crate::{check_installed, get_commit_hash_log};

/// Write file, add, commit, force push.
/// Repo must be cloned.
pub async fn write_commit_file(
  commit_msg: &str,
  repo_dir: &Path,
  // relative to repo root
  relative_file_path: &Path,
  contents: &str,
  branch: &str,
  access_token: Option<&str>,
  // TLS material for the remote, when it needs one. Only the push
  // contacts the remote, so this is the only place it matters here.
  tls: Option<&TlsAuth>,
) -> anyhow::Result<RepoExecutionResponse> {
  let mut res = RepoExecutionResponse {
    path: repo_dir.to_path_buf(),
    logs: Vec::new(),
    commit_hash: None,
    commit_message: None,
  };

  // Clean up the path by stripping any redundant `/./`
  let full_file_path = repo_dir
    .join(relative_file_path)
    .components()
    .collect::<PathBuf>();

  if let Some(parent) = full_file_path.parent() {
    tokio::fs::create_dir_all(parent).await.with_context(|| {
      format!("Failed to initialize file parent directory {parent:?}")
    })?;
  }

  tokio::fs::write(&full_file_path, contents)
    .await
    .with_context(|| {
      format!("Failed to write contents to {full_file_path:?}")
    })?;

  res.logs.push(Log::simple(
    "Write file",
    format!("File contents written to {full_file_path:?}"),
  ));

  commit_file_inner(
    commit_msg,
    &mut res,
    repo_dir,
    relative_file_path,
    branch,
    access_token,
    tls,
  )
  .await;

  Ok(res)
}

/// Add file, commit, force push.
/// Repo must be cloned.
pub async fn commit_file(
  commit_msg: &str,
  repo_dir: &Path,
  // relative to repo root
  file: &Path,
  branch: &str,
  access_token: Option<&str>,
  // TLS material for the remote, when it needs one. Only the push
  // contacts the remote, so this is the only place it matters here.
  tls: Option<&TlsAuth>,
) -> RepoExecutionResponse {
  let mut res = RepoExecutionResponse {
    path: repo_dir.to_path_buf(),
    logs: Vec::new(),
    commit_hash: None,
    commit_message: None,
  };

  commit_file_inner(
    commit_msg,
    &mut res,
    repo_dir,
    file,
    branch,
    access_token,
    tls,
  )
  .await;

  res
}

pub async fn commit_file_inner(
  commit_msg: &str,
  res: &mut RepoExecutionResponse,
  repo_dir: &Path,
  // relative to repo root
  file: &Path,
  branch: &str,
  access_token: Option<&str>,
  // TLS material for the remote, when it needs one. Only the push
  // contacts the remote, so this is the only place it matters here.
  tls: Option<&TlsAuth>,
) {
  if let Err(e) = check_installed().await {
    res
      .logs
      .push(Log::error("Commit", format_serror(&e.into())));
    return;
  };

  ensure_global_git_config_set().await;

  let add_log = run_komodo_standard_command(
    "Add Files",
    format!("git add {}", file.display()),
    CommandOptions::default().path(repo_dir),
  )
  .await;
  res.logs.push(add_log);
  if !all_logs_success(&res.logs) {
    return;
  }

  let commit_log = run_komodo_standard_command(
    "Commit",
    format!(
      r#"git commit -m "[Komodo] {commit_msg}: update {file:?}""#,
    ),
    CommandOptions::default().path(repo_dir),
  )
  .await;

  if !commit_log.success {
    // The user may have nothing to commit, but still should continue push the changes
    if !commit_log.stdout.contains("nothing to commit") {
      res.logs.push(commit_log);
      return;
    }
  } else {
    res.logs.push(commit_log);
  }

  match get_commit_hash_log(repo_dir).await {
    Ok((log, hash, message)) => {
      res.logs.push(log);
      res.commit_hash = Some(hash);
      res.commit_message = Some(message);
    }
    Err(e) => {
      res.logs.push(Log::error(
        "Get commit hash",
        format_serror(&e.into()),
      ));
      return;
    }
  };

  // The push is the only command here that contacts the remote, so it
  // is the only one needing TLS material. Built here so the session -
  // and the key file it wrote - lives exactly as long as the push.
  let tls_session = match crate::tls::TlsSession::create(
    &std::env::temp_dir(),
    &crate::ssh::safe_label(branch),
    tls.map(|tls| tls.client_cert.as_str()).unwrap_or_default(),
    tls.map(|tls| tls.client_key.as_str()).unwrap_or_default(),
    tls.map(|tls| tls.ca_bundle.as_str()).unwrap_or_default(),
  )
  .await
  {
    Ok(session) => session,
    Err(e) => {
      res.logs.push(Log::error(
        "Prepare TLS Material",
        format_serror(&e.into()),
      ));
      return;
    }
  };
  let tls_args = crate::tls::config_args(tls_session.as_ref());

  // origin is tokenless now, so push must carry its own credential -
  // it used to authenticate purely on the token sitting in .git/config.
  let push_log = run_komodo_standard_command(
    "Push",
    crate::credentials::git_command(
      access_token,
      &tls_args,
      &format!("push --set-upstream origin {branch}"),
    ),
    crate::credentials::with_credential(
      CommandOptions::default().path(repo_dir),
      access_token,
    ),
  )
  .await;

  res.logs.push(push_log);
}

/// Add, commit, and force push.
/// Repo must be cloned.
pub async fn commit_all(
  repo_dir: &Path,
  message: &str,
  branch: &str,
  access_token: Option<&str>,
  // TLS material for the remote, when it needs one. Only the push
  // contacts the remote, so this is the only place it matters here.
  tls: Option<&TlsAuth>,
) -> RepoExecutionResponse {
  let mut res = RepoExecutionResponse {
    path: repo_dir.to_path_buf(),
    logs: Vec::new(),
    commit_hash: None,
    commit_message: None,
  };

  if let Err(e) = check_installed().await {
    res
      .logs
      .push(Log::error("Commit", format_serror(&e.into())));
    return res;
  };

  ensure_global_git_config_set().await;

  let add_log = run_komodo_standard_command(
    "Add Files",
    "git add -A",
    CommandOptions::default().path(repo_dir),
  )
  .await;
  res.logs.push(add_log);
  if !all_logs_success(&res.logs) {
    return res;
  }

  let commit_log = run_komodo_standard_command(
    "Commit",
    format!(r#"git commit -m "[Komodo] {message}""#),
    CommandOptions::default().path(repo_dir),
  )
  .await;
  res.logs.push(commit_log);
  if !all_logs_success(&res.logs) {
    return res;
  }

  match get_commit_hash_log(repo_dir).await {
    Ok((log, hash, message)) => {
      res.logs.push(log);
      res.commit_hash = Some(hash);
      res.commit_message = Some(message);
    }
    Err(e) => {
      res.logs.push(Log::error(
        "Get commit hash",
        format_serror(&e.into()),
      ));
      return res;
    }
  };

  // The push is the only command here that contacts the remote, so it
  // is the only one needing TLS material. Built here so the session -
  // and the key file it wrote - lives exactly as long as the push.
  let tls_session = match crate::tls::TlsSession::create(
    &std::env::temp_dir(),
    &crate::ssh::safe_label(branch),
    tls.map(|tls| tls.client_cert.as_str()).unwrap_or_default(),
    tls.map(|tls| tls.client_key.as_str()).unwrap_or_default(),
    tls.map(|tls| tls.ca_bundle.as_str()).unwrap_or_default(),
  )
  .await
  {
    Ok(session) => session,
    Err(e) => {
      res.logs.push(Log::error(
        "Prepare TLS Material",
        format_serror(&e.into()),
      ));
      return res;
    }
  };
  let tls_args = crate::tls::config_args(tls_session.as_ref());

  // origin is tokenless now, so push must carry its own credential -
  // it used to authenticate purely on the token sitting in .git/config.
  let push_log = run_komodo_standard_command(
    "Push",
    crate::credentials::git_command(
      access_token,
      &tls_args,
      &format!("push --set-upstream origin {branch}"),
    ),
    crate::credentials::with_credential(
      CommandOptions::default().path(repo_dir),
      access_token,
    ),
  )
  .await;
  res.logs.push(push_log);

  res
}

async fn ensure_global_git_config_set() {
  let res = run_standard_command(
    "git config --global --get user.email",
    CommandOptions::default(),
  )
  .await;
  if !res.success() {
    let _ = run_standard_command(
      "git config --global user.email komodo@komo.do",
      CommandOptions::default(),
    )
    .await;
  }
  let res = run_standard_command(
    "git config --global --get user.name",
    CommandOptions::default(),
  )
  .await;
  if !res.success() {
    let _ = run_standard_command(
      "git config --global user.name komodo",
      CommandOptions::default(),
    )
    .await;
  }
}
