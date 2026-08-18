//! Prometheus exposition for the state Core already keeps in memory.
//!
//! Deliberately derived from the existing status caches rather than new
//! instrumentation: those caches are what the poll loops already
//! maintain, so scraping them adds no bookkeeping that could drift from
//! the values the UI shows.
//!
//! Unauthenticated, like `/version` - a scraper cannot present a user
//! session. It exposes resource counts and states, never names, config
//! or secrets, so the reading is "how many Servers are unreachable",
//! not "which".

use axum::{Router, response::IntoResponse, routing::get};

use crate::state::{
  action_states, cluster_status_cache, deployment_status_cache,
  server_status_cache, stack_status_cache, swarm_status_cache,
};

pub fn router() -> Router {
  Router::new().route("/", get(metrics))
}

/// One `# HELP`/`# TYPE` pair then the samples, per the Prometheus text
/// format. Labels carry the state so a scrape yields a breakdown rather
/// than a single opaque total.
struct Metrics(String);

impl Metrics {
  fn new() -> Self {
    Self(String::new())
  }

  fn gauge<'a>(
    &mut self,
    name: &str,
    help: &str,
    samples: impl IntoIterator<Item = (&'a str, usize)>,
  ) {
    let samples = samples.into_iter().collect::<Vec<_>>();
    if samples.is_empty() {
      return;
    }
    self.0.push_str(&format!("# HELP {name} {help}\n"));
    self.0.push_str(&format!("# TYPE {name} gauge\n"));
    for (label, value) in samples {
      self
        .0
        .push_str(&format!("{name}{{state=\"{label}\"}} {value}\n"));
    }
  }

  fn plain(&mut self, name: &str, help: &str, value: usize) {
    self.0.push_str(&format!("# HELP {name} {help}\n"));
    self.0.push_str(&format!("# TYPE {name} gauge\n"));
    self.0.push_str(&format!("{name} {value}\n"));
  }
}

/// Count occurrences of each state, keyed by its Debug rendering.
///
/// Debug rather than Display because these enums do not all implement
/// Display, and the name is what matters for a label. Lowercased so a
/// dashboard query does not have to know Komodo's casing.
fn tally<T: std::fmt::Debug>(
  states: impl IntoIterator<Item = T>,
) -> Vec<(String, usize)> {
  let mut counts = std::collections::BTreeMap::<String, usize>::new();
  for state in states {
    *counts
      .entry(format!("{state:?}").to_lowercase())
      .or_default() += 1;
  }
  counts.into_iter().collect()
}

async fn metrics() -> impl IntoResponse {
  let mut out = Metrics::new();

  let servers = tally(
    server_status_cache()
      .get_values()
      .await
      .into_iter()
      .map(|status| status.state),
  );
  out.gauge(
    "komodo_servers",
    "Servers by reachability state.",
    servers.iter().map(|(s, n)| (s.as_str(), *n)),
  );

  let clusters = tally(
    cluster_status_cache()
      .get_values()
      .await
      .into_iter()
      .map(|status| status.state),
  );
  out.gauge(
    "komodo_clusters",
    "Kubernetes Clusters by reachability state.",
    clusters.iter().map(|(s, n)| (s.as_str(), *n)),
  );

  let stacks = tally(
    stack_status_cache()
      .get_values()
      .await
      .into_iter()
      .map(|status| status.curr.state),
  );
  out.gauge(
    "komodo_stacks",
    "Compose Stacks by state.",
    stacks.iter().map(|(s, n)| (s.as_str(), *n)),
  );

  let deployments = tally(
    deployment_status_cache()
      .get_values()
      .await
      .into_iter()
      .map(|status| status.curr.state),
  );
  out.gauge(
    "komodo_deployments",
    "Deployments by container state.",
    deployments.iter().map(|(s, n)| (s.as_str(), *n)),
  );

  let swarms = tally(
    swarm_status_cache()
      .get_values()
      .await
      .into_iter()
      .map(|status| status.state),
  );
  out.gauge(
    "komodo_swarms",
    "Swarms by reachability state.",
    swarms.iter().map(|(s, n)| (s.as_str(), *n)),
  );

  // How many resources have an execution in flight right now - the
  // number an "is something stuck" alert wants. Read from the same
  // ActionStates the UI reads, so the two cannot disagree.
  //
  // busy() returns Result because the state is behind a Mutex; a
  // poisoned lock counts as not-busy rather than failing the whole
  // scrape, since a scrape that 500s tells an operator nothing.
  let states = action_states();
  let mut in_flight = Vec::new();
  macro_rules! busy_count {
    ($($field:ident => $label:literal),* $(,)?) => {
      $(
        let count = states
          .$field
          .get_values()
          .await
          .into_iter()
          .filter(|state| state.busy().unwrap_or(false))
          .count();
        in_flight.push(($label, count));
      )*
    };
  }
  busy_count!(
    server => "server",
    stack => "stack",
    deployment => "deployment",
    build => "build",
    repo => "repo",
    procedure => "procedure",
    action => "action",
    sync => "sync",
    cluster => "cluster",
    application => "application",
    terraform => "terraform",
    swarm => "swarm",
  );
  out.0.push_str(
    "# HELP komodo_executions_in_flight Resources with an execution currently running.\n",
  );
  out.0.push_str("# TYPE komodo_executions_in_flight gauge\n");
  for (resource, count) in in_flight {
    out.0.push_str(&format!(
      "komodo_executions_in_flight{{resource=\"{resource}\"}} {count}\n"
    ));
  }

  (
    [(
      axum::http::header::CONTENT_TYPE,
      "text/plain; version=0.0.4",
    )],
    out.0,
  )
}
