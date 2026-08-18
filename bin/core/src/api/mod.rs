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

/// The example Grafana dashboard is a separate artifact that references
/// metric names by string. Rename a metric and the dashboard does not
/// fail - it renders "No data" on every affected panel, which looks like
/// a quiet system rather than a broken query. Nothing else in the build
/// connects the two files.
///
/// So this reads the names out of the dashboard and requires each to
/// appear in the exposition source. It checks the direction that
/// actually breaks: a dashboard naming a metric Core does not emit.
/// Core may emit metrics the dashboard ignores, and that is fine.
#[cfg(test)]
mod grafana_dashboard {
  const DASHBOARD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/grafana/komodo-overview.json"
  );
  const EXPOSITION: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/src/api/metrics.rs");

  /// The dashboard is useless if it queries nothing, so a zero-name
  /// extraction is a broken parse rather than a clean pass.
  const MIN_METRIC_NAMES: usize = 5;

  /// Pull every `komodo_*` token out of the text, with the Prometheus
  /// histogram suffixes removed so `_bucket` / `_sum` / `_count` all map
  /// back to the name the source actually declares. `_total` is NOT
  /// stripped: on a counter that is part of the metric's real name.
  fn metric_names(text: &str) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(found) = text[i..].find("komodo_") {
      let start = i + found;
      let mut end = start;
      while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
      {
        end += 1;
      }
      let mut name = &text[start..end];
      for suffix in ["_bucket", "_sum", "_count"] {
        if let Some(base) = name.strip_suffix(suffix) {
          name = base;
          break;
        }
      }
      names.insert(name.to_string());
      i = end.max(start + 1);
    }
    names
  }

  #[test]
  fn every_dashboard_query_names_a_metric_core_emits() {
    let dashboard = std::fs::read_to_string(DASHBOARD).expect(
      "docs/grafana/komodo-overview.json must be readable for this \
       test to mean anything",
    );
    let exposition = std::fs::read_to_string(EXPOSITION)
      .expect("metrics.rs readable");

    let queried = metric_names(&dashboard);
    assert!(
      queried.len() >= MIN_METRIC_NAMES,
      "extracted only {} metric names from the dashboard ({queried:?}); \
       expected at least {MIN_METRIC_NAMES}. The parse is broken, so its \
       silence proves nothing.",
      queried.len(),
    );

    let emitted = metric_names(&exposition);
    let missing =
      queried.difference(&emitted).cloned().collect::<Vec<_>>();
    assert!(
      missing.is_empty(),
      "the dashboard queries metrics that {EXPOSITION} never emits, so \
       those panels will render 'No data' rather than fail: {missing:?}. \
       Emitted: {emitted:?}",
    );
  }

  /// A dashboard that will not parse cannot be imported, and nothing
  /// else in this repo parses it.
  #[test]
  fn the_dashboard_is_valid_json_with_panels() {
    let raw = std::fs::read_to_string(DASHBOARD).expect("readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw)
      .expect("dashboard must be valid JSON");
    let panels = parsed
      .get("panels")
      .and_then(|p| p.as_array())
      .expect("dashboard must have a panels array");
    assert!(
      panels.len() >= 5,
      "expected a dashboard worth shipping, found {} panels",
      panels.len()
    );
    assert!(
      parsed.get("uid").and_then(|u| u.as_str()).is_some(),
      "a uid is what makes the dashboard re-importable in place"
    );
  }
}
