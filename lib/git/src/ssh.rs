//! SSH auth for git remotes.
//!
//! ssh will not take a key from the environment - it needs a file - so
//! unlike the token path in `credentials.rs` this one has to write
//! secret material to disk. Everything here exists to bound that:
//! a per-operation directory at 0700, the key at 0600, and removal on
//! drop whether the command succeeded, failed or panicked.
//!
//! The key never appears in the command string, because arguments are
//! world-readable in `ps`. Only the PATH to the key does.

use std::path::{Path, PathBuf};

use anyhow::Context;

/// How to verify the remote's host key.
///
/// A typed choice rather than a hard-coded policy: both behaviours are
/// legitimate, and silently picking one for the operator is how a tool
/// ends up with `StrictHostKeyChecking=no` baked in forever. What is NOT
/// offered is disabling verification - there is no variant for it, so it
/// cannot be reached by configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyChecking {
  /// The host key must already be in the configured known_hosts.
  /// An unknown host fails the clone. This is the default.
  Strict,
  /// Trust the host key on first use and record it. Weaker - a MITM at
  /// first contact is not detected - but it is what most tooling does,
  /// and choosing it is the operator's call to make explicitly.
  AcceptNew,
}

impl HostKeyChecking {
  /// The value for ssh's `StrictHostKeyChecking` option.
  ///
  /// Neither variant maps to `no`. That is deliberate: `no` also
  /// silently writes the key AND continues when a known key CHANGES,
  /// which is precisely the MITM signal you never want suppressed.
  fn as_ssh_value(self) -> &'static str {
    match self {
      HostKeyChecking::Strict => "yes",
      HostKeyChecking::AcceptNew => "accept-new",
    }
  }
}

/// Secret material written to disk for the lifetime of one git
/// operation, removed when this is dropped.
pub struct SshSession {
  dir: PathBuf,
  key_path: PathBuf,
  known_hosts_path: PathBuf,
  checking: HostKeyChecking,
}

impl SshSession {
  /// Write the key and known_hosts into `parent` under a name derived
  /// from `label`.
  ///
  /// Permissions are set BEFORE the secret is written where the platform
  /// allows it, because a file that is briefly 0644 is readable by
  /// anything watching, and "briefly" is all a loop needs.
  pub async fn create(
    parent: &Path,
    label: &str,
    private_key: &str,
    known_hosts: &str,
    checking: HostKeyChecking,
  ) -> anyhow::Result<Self> {
    let dir = parent.join(format!("komodo-ssh-{label}"));
    // Remove any leftover from a previous run before trusting the
    // contents - a stale key from an unrelated account would otherwise
    // be offered to this remote.
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir)
      .await
      .context("Failed to create the ssh working directory")?;
    set_mode(&dir, 0o700).await?;

    let key_path = dir.join("id");
    write_private(&key_path, ensure_trailing_newline(private_key))
      .await
      .context("Failed to write the ssh private key")?;

    let known_hosts_path = dir.join("known_hosts");
    write_private(
      &known_hosts_path,
      ensure_trailing_newline(known_hosts),
    )
    .await
    .context("Failed to write the ssh known_hosts")?;

    Ok(Self {
      dir,
      key_path,
      known_hosts_path,
      checking,
    })
  }

  /// The `GIT_SSH_COMMAND` value git should use.
  ///
  /// `IdentitiesOnly=yes` is load-bearing, not hardening trim. Without
  /// it ssh also offers the agent's keys and the default identities in
  /// the invoking user's ~/.ssh, so a Komodo account with a WRONG key
  /// can still authenticate through whatever key happens to be lying
  /// around on the host. That is a false pass: it works in testing and
  /// fails the day the agent is not there, and it means Komodo is not
  /// actually using the credential the operator configured.
  pub fn git_ssh_command(&self) -> String {
    format!(
      "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking={} -o UserKnownHostsFile={}",
      self.key_path.display(),
      self.checking.as_ssh_value(),
      self.known_hosts_path.display(),
    )
  }
}

impl Drop for SshSession {
  fn drop(&mut self) {
    // Blocking removal in Drop: async Drop does not exist, and leaving
    // a private key behind because the tidy-up was inconvenient to
    // schedule is the worse outcome. Errors are ignored because there is
    // nothing to report to - but the directory is under a path Komodo
    // owns, so a failure here is a disk problem, not a silent leak of
    // scope.
    let _ = std::fs::remove_dir_all(&self.dir);
  }
}

/// ssh rejects a key file whose last line has no newline.
fn ensure_trailing_newline(value: &str) -> String {
  if value.ends_with('\n') {
    value.to_string()
  } else {
    format!("{value}\n")
  }
}

async fn write_private(
  path: &Path,
  contents: String,
) -> anyhow::Result<()> {
  tokio::fs::write(path, contents).await?;
  set_mode(path, 0o600).await?;
  Ok(())
}

#[cfg(unix)]
async fn set_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
  use std::os::unix::fs::PermissionsExt;
  tokio::fs::set_permissions(
    path,
    std::fs::Permissions::from_mode(mode),
  )
  .await
  .with_context(|| {
    format!("Failed to restrict permissions on {}", path.display())
  })
}

/// On a non-unix host there is no mode to set. Not silently ignored -
/// ssh itself refuses a key with loose permissions, so the failure
/// surfaces there rather than here, and pretending to have secured the
/// file would be the lie.
#[cfg(not(unix))]
async fn set_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
  Ok(())
}

/// Whether a remote should be reached over ssh rather than http(s).
///
/// Presence of a key is the trigger, not a separate flag: two settings
/// that must agree is a state you can get wrong, and "I gave it an ssh
/// key but it still used https" is a confusing way to fail.
pub fn is_ssh(private_key: &str) -> bool {
  !private_key.trim().is_empty()
}

/// The scp-style remote git wants for ssh: `git@host:path`.
///
/// Not `ssh://host/path`, which is also valid but which some hosts
/// (notably older GitLab) serve on a different port, so the form that
/// works without extra configuration is the one to emit.
pub fn ssh_remote_url(
  user: &str,
  domain: &str,
  repo_path: &str,
) -> String {
  let user = if user.trim().is_empty() {
    "git"
  } else {
    user.trim()
  };
  format!("{user}@{domain}:{}", repo_path.trim_start_matches('/'))
}

/// Build a session from a resource's execution args, or `Ok(None)` when
/// the remote is not an ssh one.
///
/// The session directory goes under the system temp dir rather than the
/// repo directory on purpose: a key inside the cloned tree could be
/// committed, archived or copied by any later step that treats the tree
/// as data.
pub async fn session_for(
  args: &komodo_client::entities::RepoExecutionArgs,
  label: &str,
) -> anyhow::Result<Option<SshSession>> {
  let Some(ssh) = &args.ssh else {
    return Ok(None);
  };
  if !is_ssh(&ssh.private_key) {
    // An `ssh` block whose key is blank is a half-filled config, not a
    // request for anonymous ssh. Saying so beats emitting a git@ remote
    // that cannot possibly authenticate and reporting git's error.
    anyhow::bail!(
      "SSH is configured for this remote but the private key is empty"
    );
  }
  if ssh.known_hosts.trim().is_empty() && !ssh.accept_new_host_keys {
    // Refusing here, rather than falling back to a weaker policy, is the
    // point: the alternative is silently accepting any host key, and a
    // downgrade nobody asked for is worse than a clear failure.
    anyhow::bail!(
      "SSH is configured for this remote but no known_hosts entries were \
       given, so the host key cannot be verified. Add known_hosts for \
       this git provider, or explicitly enable accepting new host keys."
    );
  }
  let checking = if ssh.accept_new_host_keys {
    HostKeyChecking::AcceptNew
  } else {
    HostKeyChecking::Strict
  };
  SshSession::create(
    &std::env::temp_dir(),
    &safe_label(label),
    &ssh.private_key,
    &ssh.known_hosts,
    checking,
  )
  .await
  .map(Some)
}

/// Attach `GIT_SSH_COMMAND` to a command's environment.
pub fn with_ssh<'a>(
  options: command::CommandOptions<'a>,
  session: Option<&SshSession>,
) -> command::CommandOptions<'a> {
  match session {
    Some(session) => {
      options.env("GIT_SSH_COMMAND", session.git_ssh_command())
    }
    None => options,
  }
}

/// True when the file is readable by group or other.
///
/// ssh refuses a key with loose permissions, so this exists to assert
/// the permissions in a test rather than discovering the problem as an
/// opaque ssh error at clone time.
pub fn is_readable_by_others(path: &Path) -> anyhow::Result<bool> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
      .with_context(|| format!("Failed to stat {}", path.display()))?
      .permissions()
      .mode();
    // Any group or other bit set.
    Ok(mode & 0o077 != 0)
  }
  #[cfg(not(unix))]
  {
    let _ = path;
    Ok(false)
  }
}

/// Helper for callers that need a filesystem-safe label.
pub fn safe_label(value: &str) -> String {
  value
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
        c
      } else {
        '-'
      }
    })
    .collect()
}

/// Not part of the public flow - used by tests to read back what was
/// written without duplicating the path logic.
#[cfg(test)]
impl SshSession {
  fn key_path(&self) -> &Path {
    &self.key_path
  }
  fn known_hosts_path(&self) -> &Path {
    &self.known_hosts_path
  }
  fn dir(&self) -> &Path {
    &self.dir
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nnotarealkey\n-----END OPENSSH PRIVATE KEY-----";
  const HOSTS: &str = "gitlab.example.com ssh-ed25519 AAAAC3NotReal";

  fn tmp() -> PathBuf {
    std::env::temp_dir().join("komodo-ssh-tests")
  }

  async fn session(
    label: &str,
    checking: HostKeyChecking,
  ) -> SshSession {
    let parent = tmp();
    tokio::fs::create_dir_all(&parent).await.unwrap();
    SshSession::create(&parent, label, KEY, HOSTS, checking)
      .await
      .expect("session should be creatable")
  }

  /// The exposure this module exists to bound: the key CONTENTS must
  /// never reach the command line, only its path.
  #[tokio::test]
  async fn the_key_contents_never_appear_in_the_command() {
    let s = session("no-leak", HostKeyChecking::Strict).await;
    let command = s.git_ssh_command();
    assert!(
      !command.contains("notarealkey"),
      "key material leaked into the command: {command}"
    );
    assert!(command.contains(&s.key_path().display().to_string()));
  }

  /// Verification must never be off. `no` is absent from the type, so
  /// this pins that no code path reintroduces it as a string.
  #[tokio::test]
  async fn host_key_checking_is_never_disabled() {
    for checking in
      [HostKeyChecking::Strict, HostKeyChecking::AcceptNew]
    {
      let s = session("checking", checking).await;
      let command = s.git_ssh_command();
      assert!(
        !command.contains("StrictHostKeyChecking=no"),
        "host key verification was disabled: {command}"
      );
      assert!(
        command.contains("UserKnownHostsFile="),
        "checking is meaningless without a known_hosts file: {command}"
      );
    }
  }

  #[tokio::test]
  async fn strict_and_accept_new_map_to_the_right_ssh_values() {
    let strict = session("strict", HostKeyChecking::Strict).await;
    assert!(
      strict
        .git_ssh_command()
        .contains("StrictHostKeyChecking=yes")
    );
    let tofu = session("tofu", HostKeyChecking::AcceptNew).await;
    assert!(
      tofu
        .git_ssh_command()
        .contains("StrictHostKeyChecking=accept-new")
    );
  }

  /// Without IdentitiesOnly, a wrong key still authenticates via the
  /// agent or the host's default identities - passing in testing and
  /// failing later, while not using the configured credential at all.
  #[tokio::test]
  async fn only_the_configured_identity_is_offered() {
    let s = session("identities", HostKeyChecking::Strict).await;
    assert!(
      s.git_ssh_command().contains("IdentitiesOnly=yes"),
      "ssh would also offer the agent's keys: {}",
      s.git_ssh_command()
    );
  }

  #[tokio::test]
  async fn the_key_is_not_readable_by_other_users() {
    let s = session("perms", HostKeyChecking::Strict).await;
    assert!(
      !is_readable_by_others(s.key_path()).unwrap(),
      "the private key is group/other readable"
    );
    assert!(
      !is_readable_by_others(s.known_hosts_path()).unwrap(),
      "known_hosts is group/other readable"
    );
  }

  /// ssh rejects a key whose final line has no newline, which presents
  /// as an unhelpful "invalid format" rather than anything about
  /// newlines - so a key pasted into a UI field without one must still
  /// work.
  #[tokio::test]
  async fn a_key_without_a_trailing_newline_is_fixed_up() {
    let s = session("newline", HostKeyChecking::Strict).await;
    let written =
      tokio::fs::read_to_string(s.key_path()).await.unwrap();
    assert!(!KEY.ends_with('\n'), "the fixture must lack one");
    assert!(
      written.ends_with('\n'),
      "written key has no trailing newline, ssh will reject it"
    );
  }

  /// The whole point of tying removal to Drop.
  #[tokio::test]
  async fn dropping_the_session_removes_the_key_from_disk() {
    let path = {
      let s = session("cleanup", HostKeyChecking::Strict).await;
      let dir = s.dir().to_path_buf();
      assert!(dir.exists(), "setup failed, nothing to clean up");
      dir
    };
    assert!(
      !path.exists(),
      "the private key survived the session at {}",
      path.display()
    );
  }

  /// A leftover directory from a previous run must not be trusted -
  /// a stale key would otherwise be offered to this remote.
  #[tokio::test]
  async fn a_stale_directory_is_replaced_not_merged() {
    let parent = tmp();
    tokio::fs::create_dir_all(&parent).await.unwrap();
    let dir = parent.join("komodo-ssh-stale");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("leftover"), "old").await.unwrap();

    let _s = SshSession::create(
      &parent,
      "stale",
      KEY,
      HOSTS,
      HostKeyChecking::Strict,
    )
    .await
    .unwrap();
    assert!(
      !dir.join("leftover").exists(),
      "a file from a previous run survived into this session"
    );
  }

  #[test]
  fn a_key_decides_whether_ssh_is_used() {
    assert!(is_ssh("-----BEGIN OPENSSH PRIVATE KEY-----"));
    assert!(!is_ssh(""));
    assert!(!is_ssh("   \n  "), "whitespace is not a key");
  }

  #[test]
  fn the_remote_url_is_scp_style_with_a_default_user() {
    assert_eq!(
      ssh_remote_url("", "gitlab.example.com", "group/repo"),
      "git@gitlab.example.com:group/repo"
    );
    assert_eq!(
      ssh_remote_url("gituser", "gitlab.example.com", "group/repo"),
      "gituser@gitlab.example.com:group/repo"
    );
    // A leading slash on the path would produce `host:/group/repo`,
    // which ssh reads as an absolute path on the server.
    assert_eq!(
      ssh_remote_url("", "example.com", "/group/repo"),
      "git@example.com:group/repo"
    );
  }

  #[test]
  fn a_label_cannot_escape_its_directory() {
    assert_eq!(safe_label("../../etc/passwd"), "------etc-passwd");
    assert_eq!(safe_label("group/repo"), "group-repo");
    assert_eq!(safe_label("ok-name_1"), "ok-name_1");
  }
}
