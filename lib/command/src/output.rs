use std::{
  io,
  os::unix::process::ExitStatusExt,
  process::{ExitStatus, Output},
};

#[derive(Debug, Clone)]
pub struct CommandOutput {
  pub status: ExitStatus,
  pub stdout: String,
  pub stderr: String,
}

impl CommandOutput {
  pub fn from(output: io::Result<Output>) -> Self {
    match output {
      Ok(output) => Self {
        status: output.status,
        stdout: String::from_utf8(output.stdout)
          .unwrap_or("failed to generate stdout".to_string()),
        stderr: String::from_utf8(output.stderr)
          .unwrap_or("failed to generate stderr".to_string()),
      },
      Err(e) => CommandOutput::from_err(e),
    }
  }

  pub fn from_err(e: io::Error) -> Self {
    Self {
      status: ExitStatus::from_raw(1),
      stdout: "".to_string(),
      stderr: format!("{e:?}"),
    }
  }

  pub fn from_err_message(e: String) -> Self {
    Self {
      status: ExitStatus::from_raw(1),
      stdout: "".to_string(),
      stderr: e,
    }
  }

  /// A command killed by timeout or cancel. Whatever it managed to
  /// write before it died is kept - for a cancelled `terraform apply`
  /// or `kubectl rollout`, that output is the only record of how far
  /// it got - and `reason` is appended to stderr so the log says why
  /// it stopped.
  pub fn from_killed(
    reason: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
  ) -> Self {
    let stdout = to_string_lossy(stdout);
    let mut stderr = to_string_lossy(stderr);
    if !stderr.is_empty() && !stderr.ends_with('\n') {
      stderr.push('\n');
    }
    stderr.push_str(&reason);
    Self {
      status: ExitStatus::from_raw(1),
      stdout,
      stderr,
    }
  }

  pub fn success(&self) -> bool {
    self.status.success()
  }
}

fn to_string_lossy(bytes: Vec<u8>) -> String {
  String::from_utf8(bytes)
    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into())
}
