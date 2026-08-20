//! Gate an execution on the resource's configured time windows.
//!
//! Separate from `helpers::maintenance`, which suppresses *alerts*
//! during a window. This one refuses the *work*.

use anyhow::anyhow;
use komodo_client::entities::{
  ExecutionWindows, komodo_timestamp, user::User,
};

use super::maintenance::is_maintenance_window_active;

/// `Ok(None)` to proceed, `Ok(Some(note))` when a human admin
/// overrode a closed window (the caller should log the note), `Err`
/// when the run is refused.
///
/// Service users never override: the whole point of a deny window is
/// stopping *automated* deploys, and every automated path
/// (schedules, webhooks, sync-driven deploys) runs as a synthetic
/// admin. Reading `user.admin` alone would let exactly the traffic
/// this gates straight through.
pub fn check_execution_window(
  windows: &ExecutionWindows,
  user: &User,
) -> anyhow::Result<Option<String>> {
  if windows.is_none() {
    return Ok(None);
  }
  let ts = komodo_timestamp();
  let Some(reason) = windows.blocked_reason(|window| {
    is_maintenance_window_active(window, ts)
  }) else {
    return Ok(None);
  };
  if user.admin && !User::is_service_user(&user.id) {
    return Ok(Some(format!(
      "Execution window: {reason}. Running anyway - admin override by {}.",
      user.username
    )));
  }
  Err(anyhow!(
    "Refusing to run: {reason}. Adjust the resource's execution windows, or have an admin run it."
  ))
}
