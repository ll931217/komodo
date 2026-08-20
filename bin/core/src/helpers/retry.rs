//! Retry a failed execution with exponential backoff.
//!
//! Each attempt is dispatched through the normal `/execute` path, so
//! it gets its own Update document rather than appending to the
//! failed one. The audit trail is therefore a chain of Updates, which
//! is what "attempt 2 of 3" has to look like for anyone reading the
//! resource's history later.

use std::time::Duration;

use komodo_client::entities::{
  RetryConfig,
  alert::{Alert, AlertData, SeverityLevel},
  komodo_timestamp,
  update::Update,
  user::User,
};

use crate::{
  alert::send_alerts,
  api::execute::{ExecuteRequest, inner_handler},
  state::retry_attempt_cache,
};

/// Called by an execute handler after `update.finalize()`, with the
/// request needed to run the same execution again.
///
/// Pushes the "retrying" / "giving up" line onto `update` (the caller
/// still owes the `update_update` that persists it) and spawns the
/// retry itself, so the current execution is free to return.
pub async fn maybe_retry(
  update: &mut Update,
  retry: &RetryConfig,
  request: ExecuteRequest,
  user: &User,
) {
  let (_, id) = update.target.extract_variant_id();
  let key = format!("{}:{id}", update.operation);

  if update.success {
    // The chain is over, so the next failure starts from attempt 1
    // rather than inheriting a count from whatever failed last week.
    retry_attempt_cache().remove(&key).await;
    return;
  }

  // A human stopping the run is not a transient failure, and
  // retrying one is the opposite of what Cancel was pressed for.
  if update.was_cancelled() {
    retry_attempt_cache().remove(&key).await;
    return;
  }

  if !retry.enabled || retry.limit == 0 {
    return;
  }

  let attempt =
    retry_attempt_cache().get(&key).await.unwrap_or(0) + 1;

  if attempt > retry.limit {
    retry_attempt_cache().remove(&key).await;
    update.push_error_log(
      "Retry",
      format!(
        "Giving up after {} failed {}. No further attempts.",
        retry.limit,
        if retry.limit == 1 { "retry" } else { "retries" }
      ),
    );
    let ts = komodo_timestamp();
    send_alerts(&[Alert {
      id: Default::default(),
      ts,
      resolved: true,
      level: SeverityLevel::Critical,
      target: update.target.clone(),
      data: AlertData::Custom {
        message: format!(
          "{} failed and exhausted its {} configured retries",
          update.operation, retry.limit
        ),
        details: format!(
          "Last attempt: Update {}. Retries are configured on the resource itself.",
          update.id
        ),
      },
      resolved_ts: Some(ts),
    }])
    .await;
    return;
  }

  let delay = retry.delay_seconds_for(attempt);
  retry_attempt_cache().insert(key, attempt).await;

  update.push_simple_log(
    "Retry",
    format!(
      "Failed. Retrying in {delay}s (attempt {attempt} of {}). The retry is a separate Update.",
      retry.limit
    ),
  );

  let user = user.clone();
  tokio::spawn(async move {
    tokio::time::sleep(Duration::from_secs(delay)).await;
    if let Err(e) = inner_handler(request, user).await {
      // The retry never got as far as its own Update, so there is
      // nowhere else for this to surface.
      warn!("Retry attempt {attempt} failed to dispatch | {e:#}");
    }
  });
}
