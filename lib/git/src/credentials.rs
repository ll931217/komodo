//! Handing git a credential without writing it anywhere.
//!
//! The old approach embedded `user:token@` in the remote URL and set
//! that as `origin`, which put the credential in `.git/config` on every
//! host that ever cloned the repo, readable by anyone with shell there.
//! Sanitizing the LOG output - which the callers did - does nothing
//! about the copy on disk.
//!
//! Instead `origin` stays tokenless and each command that talks to the
//! remote carries a one-shot credential helper. The token reaches git
//! through the environment, so it is in neither the remote config nor
//! the process arguments.

use command::CommandOptions;

/// Env var the helper reads the username from.
const USER_VAR: &str = "KOMODO_GIT_USERNAME";
/// Env var the helper reads the token from.
const TOKEN_VAR: &str = "KOMODO_GIT_TOKEN";

/// A git credential, split the way `remote_url` splits it: an access
/// token is either `username:token` or a bare token, which git will
/// accept under any username.
pub struct GitCredential {
  username: String,
  token: String,
}

impl GitCredential {
  pub fn new(access_token: &str) -> Self {
    match access_token.split_once(':') {
      Some((username, token)) => Self {
        username: username.trim().to_string(),
        token: token.trim().to_string(),
      },
      None => Self {
        username: "token".to_string(),
        token: access_token.trim().to_string(),
      },
    }
  }
}

/// The `-c` flags that make git use our helper and only ours.
///
/// The empty `credential.helper=` first is load-bearing: git ACCUMULATES
/// helpers from system and global config, so without resetting the list
/// a helper configured on the host could answer first and hand git the
/// wrong credential - or a stale one that fails the push and looks like
/// a permissions problem.
///
/// The helper reads from the environment rather than taking the token
/// as an argument, because arguments are world-readable in `ps`.
pub fn credential_args() -> String {
  // `${}` renders a literal `$` followed by the variable NAME, so the
  // shell git spawns for the helper expands it. Interpolating the Rust
  // constant directly would emit the name as a literal string and git
  // would authenticate as the user "KOMODO_GIT_USERNAME".
  format!(
    "-c credential.helper= -c credential.helper='!f() {{ test \"$1\" = get && \
     printf \"username=%s\\npassword=%s\\n\" \"${}\" \"${}\"; }}; f'",
    USER_VAR, TOKEN_VAR
  )
}

/// Prefix a git command with the credential helper config, if there is
/// a credential to use.
pub fn git_command(access_token: Option<&str>, rest: &str) -> String {
  match access_token {
    Some(_) => format!("git {} {rest}", credential_args()),
    None => format!("git {rest}"),
  }
}

/// Attach the credential to a command's environment.
pub fn with_credential<'a>(
  options: CommandOptions<'a>,
  access_token: Option<&str>,
) -> CommandOptions<'a> {
  let Some(token) = access_token else {
    return options;
  };
  let credential = GitCredential::new(token);
  options
    .env(USER_VAR, credential.username)
    .env(TOKEN_VAR, credential.token)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_bare_token_gets_a_placeholder_username() {
    let c = GitCredential::new("glpat-abc123");
    assert_eq!(c.username, "token");
    assert_eq!(c.token, "glpat-abc123");
  }

  #[test]
  fn a_user_colon_token_is_split() {
    let c = GitCredential::new("liangshih.lin:glpat-abc123");
    assert_eq!(c.username, "liangshih.lin");
    assert_eq!(c.token, "glpat-abc123");
  }

  /// The credential must reach git through the environment only. If it
  /// ever appears in the command string it is readable from `ps` by any
  /// user on the host, which is the exposure this module exists to
  /// remove.
  #[test]
  fn the_token_never_appears_in_the_command() {
    let token = "glpat-supersecret";
    let command = git_command(Some(token), "fetch --all --prune");
    assert!(
      !command.contains(token),
      "token leaked into the command: {command}"
    );
    assert!(command.contains("credential.helper"));
    // The helper must reference the env vars, not embed their names as
    // literal values - the difference between reading the token and
    // authenticating as the string "KOMODO_GIT_USERNAME".
    assert!(
      command.contains("\"$KOMODO_GIT_USERNAME\"")
        && command.contains("\"$KOMODO_GIT_TOKEN\""),
      "helper does not expand the env vars: {command}"
    );
  }

  /// Resetting the helper list first is what stops a helper configured
  /// on the host from answering instead of ours.
  #[test]
  fn the_helper_list_is_reset_before_ours_is_added() {
    let args = credential_args();
    let reset = args.find("credential.helper= ").expect("no reset");
    let ours =
      args.find("credential.helper='!f()").expect("no helper");
    assert!(reset < ours, "ours must come after the reset: {args}");
  }

  /// Without a token the command must be plain git - no helper, no
  /// stray config that would change behaviour for public repos.
  #[test]
  fn no_token_means_a_plain_git_command() {
    assert_eq!(git_command(None, "fetch --all"), "git fetch --all");
  }
}
