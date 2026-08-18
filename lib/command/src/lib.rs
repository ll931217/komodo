use std::{path::PathBuf, process::Stdio, sync::OnceLock};

use komodo_client::{
  entities::{komodo_timestamp, update::Log},
  parsers::parse_multiline_command,
};
use tokio::{
  io::{AsyncRead, AsyncReadExt},
  process::Command,
  task::JoinHandle,
};

mod options;
mod output;

pub use options::*;
pub use output::*;

/// Commands are run directly, and cannot include '&&'
pub async fn run_komodo_standard_command(
  stage: &str,
  command: impl Into<String>,
  options: CommandOptions<'_>,
) -> Log {
  let command = command.into();
  let start_ts = komodo_timestamp();
  let output = run_standard_command(&command, options).await;
  output_into_log(stage, command, start_ts, output)
}

/// Commands are wrapped in 'sh -c', and can include '&&'
pub async fn run_komodo_shell_command(
  stage: &str,
  command: impl Into<String>,
  options: CommandOptions<'_>,
) -> Log {
  let command = command.into();
  let start_ts = komodo_timestamp();
  let output = run_shell_command(&command, options).await;
  output_into_log(stage, command, start_ts, output)
}

/// Parses commands out of multiline string
/// and chains them together with '&&'.
/// Supports full line and end of line comments.
/// See [parse_multiline_command].
///
/// The result may be None if the command is empty after parsing,
/// ie if all the lines are commented out.
pub async fn run_komodo_multiline_command(
  stage: &str,
  command: impl AsRef<str>,
  options: CommandOptions<'_>,
) -> Option<Log> {
  let command = parse_multiline_command(command);
  if command.is_empty() {
    return None;
  }
  Some(run_komodo_shell_command(stage, command, options).await)
}

pub enum KomodoCommandMode {
  Standard,
  Shell,
  Multiline,
}

/// Executes the command, and sanitizes the output to avoid exposing secrets in the log.
///
/// Checks to make sure the command is non-empty after being multiline-parsed.
///
/// If `parse_multiline: true`, parses commands out of multiline string
/// and chains them together with '&&'.
/// Supports full line and end of line comments.
/// See [parse_multiline_command].
pub async fn run_komodo_command_with_sanitization(
  stage: &str,
  command: impl AsRef<str>,
  options: CommandOptions<'_>,
  mode: KomodoCommandMode,
  replacers: &[(String, String)],
) -> Option<Log> {
  let mut log = match mode {
    KomodoCommandMode::Standard => {
      run_komodo_standard_command(stage, command.as_ref(), options)
        .await
        .into()
    }
    KomodoCommandMode::Shell => {
      run_komodo_shell_command(stage, command.as_ref(), options)
        .await
        .into()
    }
    KomodoCommandMode::Multiline => {
      run_komodo_multiline_command(stage, command, options).await
    }
  }?;

  // Sanitize the command and output
  log.command = svi::replace_in_string(&log.command, replacers);
  log.stdout = svi::replace_in_string(&log.stdout, replacers);
  log.stderr = svi::replace_in_string(&log.stderr, replacers);

  Some(log)
}

pub fn output_into_log(
  stage: &str,
  command: String,
  start_ts: i64,
  output: CommandOutput,
) -> Log {
  let success = output.success();
  Log {
    stage: stage.to_string(),
    stdout: output.stdout,
    stderr: output.stderr,
    command,
    success,
    start_ts,
    end_ts: komodo_timestamp(),
  }
}

/// Commands are run directly, and cannot include '&&'.
///
/// See [CommandOptions] for the available `timeout` / `cancel` / `stdin`
/// controls.
pub async fn run_standard_command(
  command: &str,
  options: CommandOptions<'_>,
) -> CommandOutput {
  let lexed = if let Some(lexed) = shlex::split(command)
    && !lexed.is_empty()
  {
    lexed
  } else {
    return CommandOutput::from_err(std::io::Error::other(
      "Command lexed into empty args",
    ));
  };

  let mut cmd = Command::new(&lexed[0]);

  cmd.args(&lexed[1..]).kill_on_drop(true);

  run_command(cmd, options).await
}

fn shell() -> &'static str {
  static DEFAULT_SHELL: OnceLock<String> = OnceLock::new();
  DEFAULT_SHELL.get_or_init(|| {
    if PathBuf::from("/bin/bash").exists() {
      String::from("/bin/bash")
    } else if PathBuf::from("/usr/bin/bash").exists() {
      String::from("/usr/bin/bash")
    } else if PathBuf::from("/bin/sh").exists() {
      String::from("/bin/sh")
    } else if PathBuf::from("/usr/bin/sh").exists() {
      String::from("/usr/bin/sh")
    } else {
      // try to use sh wherever it is on host by name.
      String::from("sh")
    }
  })
}

/// Commands are wrapped in 'sh -c', and can include '&&'.
///
/// See [CommandOptions] for the available `timeout` / `cancel` / `stdin`
/// controls.
pub async fn run_shell_command(
  command: &str,
  options: CommandOptions<'_>,
) -> CommandOutput {
  let mut cmd = Command::new(shell());

  cmd.args(["-c", command]).kill_on_drop(true);

  run_command(cmd, options).await
}

/// Runs the command to completion, returning its output.
///
/// With an empty `options`, this is just `cmd.output()`.
///
/// With a `timeout` and/or `cancel` token, the child is spawned in its own
/// process group (via `process_group(0)`, so the child's pid is also its
/// process group id). If the timeout elapses or the token is cancelled
/// before the command finishes, the entire process group is killed with
/// `SIGKILL` — not just the direct child — so any descendants the command
/// spawned (e.g. processes started by a `sh -c` wrapper) are torn down too.
/// `kill_on_drop(true)` remains set as a backstop to reap the direct child.
async fn run_command(
  mut cmd: Command,
  options: CommandOptions<'_>,
) -> CommandOutput {
  let CommandOptions {
    path,
    timeout,
    cancel,
    stdin,
    env,
  } = options;

  for (key, value) in &env {
    cmd.env(key, value);
  }

  // Attach the path to cmd as current dir
  if let Some(path) = path {
    match path.canonicalize() {
      Ok(path) => {
        cmd.current_dir(path);
      }
      Err(e) => return CommandOutput::from_err(e),
    }
  }

  // Anything to write needs a pipe, otherwise the command gets no input.
  cmd.stdin(if stdin.is_some() {
    Stdio::piped()
  } else {
    Stdio::null()
  });

  // Fast path: nothing to write, and nothing to wait
  // on besides the command itself.
  if timeout.is_none() && cancel.is_none() && stdin.is_none() {
    return CommandOutput::from(cmd.output().await);
  }

  // `output()` configures stdout/stderr automatically, but since
  // we spawn manually we set them here too.
  cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

  // Place the child in a new process group so the whole group can be
  // signalled together. Only needed when there is something to signal on.
  if timeout.is_some() || cancel.is_some() {
    cmd.process_group(0);
  }

  let mut child = match cmd.spawn() {
    Ok(child) => child,
    Err(e) => return CommandOutput::from_err(e),
  };

  // Written from a task, so that a payload larger than the pipe buffer
  // cannot deadlock against a child which is filling up its stdout.
  if let Some(stdin) = stdin
    && let Some(mut pipe) = child.stdin.take()
  {
    let stdin = stdin.to_string();
    tokio::spawn(async move {
      use tokio::io::AsyncWriteExt;
      let _ = pipe.write_all(stdin.as_bytes()).await;
      // Dropping the pipe closes the child's stdin, so it stops reading.
      let _ = pipe.shutdown().await;
    });
  }

  // Because of `process_group(0)`, the child's pid equals its pgid.
  let pid = child.id();

  // Nothing to signal on, so just wait for it to finish.
  if timeout.is_none() && cancel.is_none() {
    return CommandOutput::from(child.wait_with_output().await);
  }

  // Resolve to `()` only when the relevant control fires; otherwise stay
  // pending forever so it never wins the `select!`.
  let on_timeout = async {
    match timeout {
      Some(timeout) => tokio::time::sleep(timeout).await,
      None => std::future::pending().await,
    }
  };
  let on_cancel = async {
    match &cancel {
      Some(cancel) => cancel.cancelled().await,
      None => std::future::pending().await,
    }
  };

  // Drain the pipes while the command runs, instead of only through
  // `wait_with_output`. If the command is killed that call never
  // returns, so everything it had already printed was thrown away -
  // and for a cancelled `terraform apply` that output is exactly what
  // the user needs to see.
  let stdout = spawn_reader(child.stdout.take());
  let stderr = spawn_reader(child.stderr.take());

  let killed_reason = tokio::select! {
    status = child.wait() => {
      return match status {
        Ok(status) => CommandOutput::from(Ok(std::process::Output {
          status,
          stdout: collect_reader(stdout).await,
          stderr: collect_reader(stderr).await,
        })),
        Err(e) => CommandOutput::from_err(e),
      };
    }
    _ = on_timeout => format!(
      "Command timed out after {:.1}s (process group killed)",
      // `timeout` is `Some` here, since `on_timeout` only fires when set.
      timeout.map(|t| t.as_secs_f64()).unwrap_or_default(),
    ),
    _ = on_cancel => {
      "Command cancelled (process group killed)".to_string()
    }
  };

  kill_process_group(pid);
  CommandOutput::from_killed(
    killed_reason,
    collect_killed_reader(stdout).await,
    collect_killed_reader(stderr).await,
  )
}

/// Reads a child pipe to EOF in the background.
///
/// Two reasons this runs as its own task: a command that outruns the
/// pipe buffer cannot block on a reader that only runs at the end, and
/// what it wrote is already in hand if the command is killed before it
/// exits.
fn spawn_reader<R>(pipe: Option<R>) -> Option<JoinHandle<Vec<u8>>>
where
  R: AsyncRead + Unpin + Send + 'static,
{
  pipe.map(|mut pipe| {
    tokio::spawn(async move {
      let mut buf = Vec::new();
      // A read error still leaves everything read up to it in `buf`,
      // which is the same partial-output guarantee as a kill.
      if let Err(e) = pipe.read_to_end(&mut buf).await {
        tracing::debug!("command pipe read ended early: {e:?}");
      }
      buf
    })
  })
}

async fn collect_reader(
  reader: Option<JoinHandle<Vec<u8>>>,
) -> Vec<u8> {
  match reader {
    Some(reader) => reader.await.unwrap_or_default(),
    None => Vec::new(),
  }
}

/// Same, but after the process group was killed.
///
/// EOF arrives when the last writer closes, so a descendant that left
/// the group (`setsid`) still holds the pipe and this would never
/// return. Waiting forever to collect output is worse than losing it.
async fn collect_killed_reader(
  reader: Option<JoinHandle<Vec<u8>>>,
) -> Vec<u8> {
  // ponytail: fixed 2s grace. Make it configurable only if a real
  // command is found that needs longer to flush after SIGKILL.
  tokio::time::timeout(
    std::time::Duration::from_secs(2),
    collect_reader(reader),
  )
  .await
  .unwrap_or_default()
}

/// Sends `SIGKILL` to the entire process group led by `pid`.
///
/// A negative pid targets the whole group, so a child spawned with
/// `process_group(0)` (group leader) is killed along with all of its
/// descendants.
fn kill_process_group(pid: Option<u32>) {
  let Some(pid) = pid else {
    return;
  };
  // SAFETY: `kill` is a simple syscall with no memory safety concerns;
  // we only signal our own child's process group.
  unsafe {
    libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use tokio_util::sync::CancellationToken;

  use super::*;

  /// On timeout, a backgrounded grandchild (here `sleep 31337`, started
  /// with `&` so it is not the direct child of our spawned shell) must be
  /// killed along with the rest of the process group.
  #[tokio::test]
  async fn timeout_kills_process_group() {
    // Unique sleep duration so we can pgrep for exactly this process.
    let marker = "sleep 31337";
    let out = run_shell_command(
      &format!("{marker} & sleep 31336"),
      CommandOptions::default().timeout(Duration::from_millis(300)),
    )
    .await;

    // The command should have reported a timeout failure.
    assert!(!out.success(), "expected timeout failure, got: {out:?}");
    assert!(
      out.stderr.contains("timed out"),
      "expected timeout error, got: {out:?}"
    );

    // Give the kernel a moment to reap the killed group.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let still_running = std::process::Command::new("pgrep")
      .args(["-f", marker])
      .output()
      .expect("pgrep should run");
    let pids = String::from_utf8_lossy(&still_running.stdout);
    let pids = pids.trim();
    assert!(
      pids.is_empty(),
      "backgrounded grandchild survived timeout: pids={pids:?}"
    );
  }

  /// Neither runner configures stdout/stderr itself — the fast path relies
  /// on `output()` doing it, and the spawn path on [run_command]. Both must
  /// still capture, rather than leaking output to the parent.
  #[tokio::test]
  async fn output_is_captured_on_both_spawn_paths() {
    // Fast path: no stdin, timeout or cancel, so `cmd.output()`.
    let fast =
      run_standard_command("echo out", CommandOptions::default())
        .await;
    assert!(fast.success(), "{fast:?}");
    assert_eq!(fast.stdout.trim(), "out");

    // Spawn path, reached here by setting a timeout.
    let spawned = run_shell_command(
      "echo out; echo err >&2",
      CommandOptions::default().timeout(Duration::from_secs(10)),
    )
    .await;
    assert!(spawned.success(), "{spawned:?}");
    assert_eq!(spawned.stdout.trim(), "out");
    assert_eq!(spawned.stderr.trim(), "err");
  }

  /// Data set as [CommandOptions::stdin] must reach the child on its
  /// standard input.
  #[tokio::test]
  async fn stdin_is_delivered_to_child() {
    let payload = "some-secret-token\nwith two lines";
    let out = run_standard_command(
      "cat",
      CommandOptions::default().stdin(payload),
    )
    .await;

    assert!(out.success(), "cat failed: {out:?}");
    assert_eq!(out.stdout, payload);
  }

  /// The whole point of [CommandOptions::stdin]: a secret passed this way
  /// must not be visible in the process arguments, where any user on the
  /// host could read it with `ps`. The shell equivalent, which interpolates
  /// the secret into the command, is checked alongside it to confirm the
  /// test would actually catch a regression.
  #[tokio::test]
  async fn stdin_secret_is_not_in_process_args() {
    let secret = "komodo-stdin-secret-marker";

    let visible_in_ps = |marker: &'static str| async move {
      // Give the child a moment to actually be running.
      tokio::time::sleep(Duration::from_millis(300)).await;
      let out = std::process::Command::new("pgrep")
        .args(["-f", marker])
        .output()
        .expect("pgrep should run");
      !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    };

    let (_, via_stdin) = tokio::join!(
      run_standard_command(
        "sleep 1",
        CommandOptions::default().stdin(secret)
      ),
      visible_in_ps(secret)
    );
    assert!(!via_stdin, "secret passed via stdin showed up in ps");

    // Control: interpolating it into the command does expose it.
    let interpolated = format!("echo {secret} | sleep 1");
    let (_, via_command) = tokio::join!(
      run_shell_command(&interpolated, CommandOptions::default()),
      visible_in_ps(secret)
    );
    assert!(
      via_command,
      "control failed: interpolated secret should be visible in ps"
    );
  }

  /// Cancelling the token mid-run must kill the whole process group, just
  /// like a timeout does, and return promptly with a cancellation error.
  #[tokio::test]
  async fn cancel_kills_process_group() {
    let marker = "sleep 41337";
    let cancel = CancellationToken::new();

    // Cancel shortly after the command starts.
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
      tokio::time::sleep(Duration::from_millis(300)).await;
      cancel_clone.cancel();
    });

    let out = run_shell_command(
      &format!("{marker} & sleep 41336"),
      CommandOptions::default().cancel(cancel),
    )
    .await;

    assert!(!out.success(), "expected cancel failure, got: {out:?}");
    assert!(
      out.stderr.contains("cancelled"),
      "expected cancellation error, got: {out:?}"
    );

    tokio::time::sleep(Duration::from_millis(200)).await;

    let still_running = std::process::Command::new("pgrep")
      .args(["-f", marker])
      .output()
      .expect("pgrep should run");
    let pids = String::from_utf8_lossy(&still_running.stdout);
    let pids = pids.trim();
    assert!(
      pids.is_empty(),
      "backgrounded grandchild survived cancellation: pids={pids:?}"
    );
  }

  /// A cancelled `terraform apply` has usually printed the part that
  /// matters before the user stops it. Killing the process group must
  /// not throw that away.
  #[tokio::test]
  async fn cancel_keeps_the_output_the_command_already_produced() {
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
      tokio::time::sleep(Duration::from_millis(400)).await;
      cancel_clone.cancel();
    });

    let out = run_shell_command(
      "echo made-progress; echo warned-about-it >&2; sleep 41338",
      CommandOptions::default().cancel(cancel),
    )
    .await;

    assert!(!out.success(), "expected cancel failure, got: {out:?}");
    assert!(
      out.stdout.contains("made-progress"),
      "stdout written before the kill was lost: {out:?}"
    );
    assert!(
      out.stderr.contains("warned-about-it"),
      "stderr written before the kill was lost: {out:?}"
    );
    assert!(
      out.stderr.contains("cancelled"),
      "the kill reason should follow the captured output: {out:?}"
    );
  }

  /// A secret handed over as an env var must actually reach the child,
  /// and must NOT be reconstructable from the command line - that is the
  /// entire reason for preferring it over interpolating into the string.
  #[tokio::test]
  async fn env_reaches_the_child_without_entering_the_command_line() {
    let secret = "s3cr3t-not-in-argv";
    let log = run_komodo_shell_command(
      "Env",
      "printf '%s' \"$KOMODO_TEST_TOKEN\"",
      CommandOptions::default().env("KOMODO_TEST_TOKEN", secret),
    )
    .await;

    assert!(log.success, "command failed: {log:?}");
    assert_eq!(
      log.stdout, secret,
      "the child did not receive the env var"
    );
    assert!(
      !log.command.contains(secret),
      "the secret leaked into the command line: {}",
      log.command
    );
  }

  /// Same guarantee on the timeout path, which shares the kill branch.
  #[tokio::test]
  async fn timeout_keeps_the_output_the_command_already_produced() {
    let out = run_shell_command(
      "echo got-this-far; sleep 31338",
      CommandOptions::default().timeout(Duration::from_millis(400)),
    )
    .await;

    assert!(!out.success(), "expected timeout failure, got: {out:?}");
    assert!(
      out.stdout.contains("got-this-far"),
      "stdout written before the timeout was lost: {out:?}"
    );
    assert!(
      out.stderr.contains("timed out"),
      "the kill reason should follow the captured output: {out:?}"
    );
  }
}
