use axum::{Extension, Router, routing::get};
use komodo_client::entities::user::User;
use mogh_auth_server::middleware::authenticate_request;
use mogh_error::Json;
use mogh_server::{
  cors::cors_layer, session::memory_session_layer,
  ui::serve_static_ui,
};

use crate::{auth::KomodoAuthImpl, config::core_config, ts_client};

pub mod execute;
pub mod read;
pub mod write;

mod listener;
mod metrics;
mod openapi;
mod terminal;
mod ws;

#[derive(serde::Deserialize)]
struct Variant {
  variant: String,
}

pub fn app() -> Router {
  let config = core_config();
  Router::new()
    .merge(openapi::serve_docs())
    .route("/version", get(|| async { env!("CARGO_PKG_VERSION") }))
    // Unauthenticated like /version: a Prometheus scraper cannot hold a
    // user session. Exposes counts and states only - never names,
    // config or secrets.
    .nest("/metrics", metrics::router())
    .nest("/auth", mogh_auth_server::api::router::<KomodoAuthImpl>())
    .nest("/user", user_router())
    .nest("/read", read::router())
    .nest("/write", write::router())
    .nest("/execute", execute::router())
    .nest("/kubernetes", crate::kubernetes::router())
    .nest("/terminal", terminal::router())
    .nest("/listener", listener::router())
    .nest("/ws", ws::router())
    .nest("/client", ts_client::router())
    .layer(memory_session_layer(config))
    .fallback_service(serve_static_ui(
      &config.ui_path,
      config.ui_index_force_no_cache,
    ))
    .layer(cors_layer(config))
}

fn user_router() -> Router {
  Router::new()
    .route(
      "/",
      get(|Extension(user): Extension<User>| async { Json(user) }),
    )
    .layer(axum::middleware::from_fn(
      authenticate_request::<KomodoAuthImpl, false>,
    ))
}

/// A cancelled run has to be tellable from a failed one, which
/// `Update::was_cancelled` decides by looking for a log whose stage is
/// exactly `CANCELLED_LOG_STAGE`. That makes the marker a convention
/// rather than something the type system enforces: a new cancel path can
/// log `"Build Cancelled"` instead, compile, ship, and silently report
/// `was_cancelled() == false` forever.
///
/// That is not hypothetical - it is what build.rs and repo.rs did. Two
/// cancel paths, three different spellings between them ("Build
/// cancelled", "Build Cancelled", "build cancelled"), none of them the
/// constant. They were missed precisely because the search that found
/// the other four looked for *users of the constant*, which by
/// construction cannot find a path that never adopted it.
///
/// So this scans for the real axis instead - files that KNOW about
/// cancellation - and requires each to reference the constant.
///
/// Deliberately lives in `api/mod.rs`, one level above the directory it
/// reads: a scan whose scope contains its own source matches its own
/// literals, and would keep passing on nothing but itself.
#[cfg(test)]
mod cancelled_convention {
  const EXECUTE_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/src/api/execute");

  /// Below this, assume the scan broke rather than that the codebase
  /// stopped cancelling things. A bare zero-hit is equally consistent
  /// with "all clean" and "read the wrong directory".
  const MIN_CANCEL_AWARE_FILES: usize = 6;

  #[test]
  fn every_cancel_path_marks_the_update_cancelled() {
    let mut cancel_aware = Vec::new();
    let mut offenders = Vec::new();

    for entry in std::fs::read_dir(EXECUTE_DIR).expect(
      "execute/ must be readable for this test to mean anything",
    ) {
      let path = entry.expect("readable dir entry").path();
      if path.extension().and_then(|e| e.to_str()) != Some("rs") {
        continue;
      }
      let source =
        std::fs::read_to_string(&path).expect("readable file");
      // Bare suffix on purpose, so it catches both spellings:
      // `cancel.cancelled().await` in a select! arm (build, repo) and
      // `cancel.is_cancelled()` polled after a step (the other four).
      // Matching ".cancelled()" would silently skip the poll form.
      if !source.contains("cancelled()") {
        continue;
      }
      let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .expect("utf8 filename")
        .to_string();
      cancel_aware.push(name.clone());
      if !source.contains("CANCELLED_LOG_STAGE") {
        offenders.push(name);
      }
    }

    assert!(
      cancel_aware.len() >= MIN_CANCEL_AWARE_FILES,
      "found only {} cancel-aware files in {EXECUTE_DIR} ({cancel_aware:?}); \
       expected at least {MIN_CANCEL_AWARE_FILES}. The scan is broken, so \
       its silence proves nothing.",
      cancel_aware.len(),
    );

    assert!(
      offenders.is_empty(),
      "these handle cancellation but never reference CANCELLED_LOG_STAGE, \
       so Update::was_cancelled() reports false for runs they cancel - \
       indistinguishable from a plain failure: {offenders:?}",
    );
  }
}
