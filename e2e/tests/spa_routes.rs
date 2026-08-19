//! SPA deep routes answer 200, and only SPA deep routes do.
//!
//! The upstream static handler serves index.html for any unmatched path
//! - correct, since the browser router resolves it - but wraps it in
//! SetStatus(404), so every bookmarked deep link answered 404 while
//! returning a perfectly good page. Uptime checks and link checkers
//! report those as broken.
//!
//! The negative test is the one that matters. Rewriting 404 -> 200 too
//! broadly would make a BROKEN DEPLOYMENT look healthy: a renamed bundle
//! chunk, a deleted image, a mistyped asset path would all start
//! answering 200, and nothing would notice a half-shipped release.

// Integration test targets link every package dependency,
// tripping -Wunused-crate-dependencies for deps only the lib uses.
#![allow(unused_crate_dependencies)]

use komodo_e2e::e2e_env;

async fn status_and_type(path: &str) -> Option<(u16, String)> {
  let env = e2e_env()?;
  let response = reqwest::Client::new()
    .get(format!("{}{path}", env.address))
    .send()
    .await
    .ok()?;
  let status = response.status().as_u16();
  let content_type = response
    .headers()
    .get(reqwest::header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .unwrap_or_default()
    .to_string();
  Some((status, content_type))
}

/// A client-side route is not a missing page.
///
/// Skips when Core is running without a built frontend, which is how
/// the e2e stack runs it - there is no index.html to serve, so `/`
/// itself 404s and there is no SPA shell for a deep route to return.
/// The rewrite cannot be exercised without one, and asserting anyway
/// would be testing the fixture rather than the behaviour.
#[tokio::test]
async fn spa_deep_routes_answer_ok() {
  let Some((root_status, root_type)) = status_and_type("/").await
  else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  if !root_type.starts_with("text/html") {
    eprintln!(
      "Core is serving no UI (root is {root_status} {root_type}), \
       skipping - there is no SPA shell to rewrite"
    );
    return;
  }
  assert_eq!(root_status, 200, "the root must serve the app");

  for path in ["/stacks", "/all-resources", "/stacks/does-not-exist"]
  {
    let (status, content_type) =
      status_and_type(path).await.expect("env checked above");
    assert!(
      content_type.starts_with("text/html"),
      "{path} should serve the SPA shell, got {content_type}"
    );
    assert_eq!(
      status, 200,
      "{path} is a client-side route, not a missing page - a 404 here \
       makes every bookmarked link read as broken to uptime checks and \
       link checkers"
    );
  }
}

/// The guard. A genuinely missing FILE must still 404, or a broken
/// deployment - a renamed chunk, a deleted asset - starts looking
/// healthy to everything that checks.
#[tokio::test]
async fn a_missing_asset_still_answers_not_found() {
  let Some((status, content_type)) =
    status_and_type("/assets/index-DoesNotExist00000.js").await
  else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  assert_ne!(
    content_type.split(';').next().unwrap_or_default(),
    "text/html",
    "a missing .js must not be answered with the SPA shell"
  );
  assert_eq!(
    status, 404,
    "a missing asset must keep its 404. Rewriting it would make a \
     half-shipped release - a renamed bundle chunk - indistinguishable \
     from a working one."
  );
}

/// An unknown API path answers JSON, so it must keep its status too.
#[tokio::test]
async fn an_unknown_api_route_keeps_its_status() {
  let Some((status, _)) =
    status_and_type("/auth/definitely-not-a-real-endpoint").await
  else {
    eprintln!("KOMODO_ADDRESS not set, skipping");
    return;
  };
  assert_ne!(
    status, 200,
    "an unknown API route must not answer 200 - a client cannot tell a \
     typo'd endpoint from a working one"
  );
}
