//! Timing for the git operations that talk to a remote.
//!
//! This is the one place in Komodo that adds instrumentation rather than
//! reading state something else already maintains, because nothing times
//! a git fetch today. It is kept deliberately small: three counters and a
//! bucket array per operation, no registry crate, no background task.
//!
//! **Only remote-contacting commands are timed** - `clone`, `fetch` and
//! `pull`. Local steps in the same functions (checkout, reset, reading
//! the latest commit) are excluded, because mixing them in would make a
//! slow-network diagnosis impossible: the number would move for reasons
//! that have nothing to do with the remote.
//!
//! **A cached pull is not observed.** `pull` short-circuits within
//! PULL_TIMEOUT and returns without contacting anything; counting those
//! would pull the distribution toward zero and quietly understate real
//! fetch latency. Observation happens at the command, not at the
//! function boundary, so a cache hit records nothing by construction.
//!
//! Scope worth stating: these are PROCESS-local. `lib/git` is linked by
//! both Core and Periphery, so each accumulates its own; Core's
//! `/metrics` therefore reports Core's fetches (syncs, stacks, repos,
//! builds reading remote config), not Periphery's clones. Periphery
//! exposes no scrape endpoint to report them on.

use std::{
  sync::atomic::{AtomicU64, Ordering},
  time::Duration,
};

/// Upper bounds in seconds. Chosen for the shape git actually has: a
/// warm fetch against a nearby remote is sub-second, a cold clone of a
/// large repo is tens of seconds, and anything past a minute is the
/// interesting case (a hung remote, a bad credential path) rather than
/// noise worth resolving finely.
pub const BUCKETS: [f64; 9] =
  [0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];

/// The remote-contacting git operations, in the order they are reported.
#[derive(Debug, Clone, Copy)]
pub enum GitOp {
  Clone,
  Fetch,
  Pull,
}

impl GitOp {
  pub const ALL: [GitOp; 3] =
    [GitOp::Clone, GitOp::Fetch, GitOp::Pull];

  /// The Prometheus label value.
  pub fn as_str(self) -> &'static str {
    match self {
      GitOp::Clone => "clone",
      GitOp::Fetch => "fetch",
      GitOp::Pull => "pull",
    }
  }

  fn index(self) -> usize {
    match self {
      GitOp::Clone => 0,
      GitOp::Fetch => 1,
      GitOp::Pull => 2,
    }
  }
}

/// Atomics rather than a Mutex, so a scrape can never be blocked by an
/// in-flight fetch and a panicking fetch cannot poison the metrics.
/// `Relaxed` throughout: these are independent counters, and a scrape
/// that catches one increment before its sibling is off by one sample
/// for one scrape interval - which does not change any decision anyone
/// makes from a latency histogram.
struct OpState {
  /// Non-cumulative per-bucket hits, plus a final +Inf overflow slot.
  buckets: [AtomicU64; BUCKETS.len() + 1],
  count: AtomicU64,
  /// Micros, so the sum stays an integer.
  sum_micros: AtomicU64,
  failures: AtomicU64,
}

impl OpState {
  const fn new() -> Self {
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicU64 = AtomicU64::new(0);
    Self {
      buckets: [ZERO; BUCKETS.len() + 1],
      count: ZERO,
      sum_micros: ZERO,
      failures: ZERO,
    }
  }
}

static STATE: [OpState; 3] =
  [OpState::new(), OpState::new(), OpState::new()];

/// Record one completed remote git operation.
///
/// `success` is the command's own success, so a git failure (bad
/// credential, unreachable host) still contributes its duration - a
/// timeout is exactly the latency you want to see, and dropping failures
/// would hide the worst cases from the histogram.
pub fn observe(op: GitOp, success: bool, elapsed: Duration) {
  let state = &STATE[op.index()];
  let seconds = elapsed.as_secs_f64();

  let bucket = BUCKETS
    .iter()
    .position(|bound| seconds <= *bound)
    .unwrap_or(BUCKETS.len());
  state.buckets[bucket].fetch_add(1, Ordering::Relaxed);

  state.count.fetch_add(1, Ordering::Relaxed);
  state
    .sum_micros
    .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
  if !success {
    state.failures.fetch_add(1, Ordering::Relaxed);
  }
}

/// One operation's observations, buckets already made cumulative as the
/// Prometheus histogram format requires.
pub struct OpSnapshot {
  pub op: GitOp,
  /// `(le, cumulative_count)`, ending with the `+Inf` bucket.
  pub cumulative: Vec<(String, u64)>,
  pub count: u64,
  pub sum_seconds: f64,
  pub failures: u64,
}

pub fn snapshot() -> Vec<OpSnapshot> {
  GitOp::ALL
    .into_iter()
    .map(|op| {
      let state = &STATE[op.index()];
      let mut running = 0u64;
      let mut cumulative = Vec::with_capacity(BUCKETS.len() + 1);
      for (i, bound) in BUCKETS.iter().enumerate() {
        running += state.buckets[i].load(Ordering::Relaxed);
        cumulative.push((format!("{bound}"), running));
      }
      running += state.buckets[BUCKETS.len()].load(Ordering::Relaxed);
      cumulative.push(("+Inf".to_string(), running));

      OpSnapshot {
        op,
        cumulative,
        count: state.count.load(Ordering::Relaxed),
        sum_seconds: state.sum_micros.load(Ordering::Relaxed) as f64
          / 1_000_000.0,
        failures: state.failures.load(Ordering::Relaxed),
      }
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The +Inf bucket must equal the total count, or a dashboard's
  /// quantile maths is wrong. This is the invariant most easily broken
  /// by adding a bucket bound and forgetting the overflow slot.
  #[test]
  fn the_inf_bucket_accounts_for_every_observation() {
    observe(GitOp::Pull, true, Duration::from_millis(5));
    observe(GitOp::Pull, true, Duration::from_secs(3));
    // Past the last bound, so it can only land in the overflow slot.
    observe(GitOp::Pull, false, Duration::from_secs(600));

    let snap = snapshot()
      .into_iter()
      .find(|s| matches!(s.op, GitOp::Pull))
      .expect("pull is one of GitOp::ALL");

    let (label, inf) = snap.cumulative.last().expect("has buckets");
    assert_eq!(label, "+Inf");
    assert_eq!(
      *inf, snap.count,
      "the +Inf bucket ({inf}) must equal count ({}); an observation \
       fell outside every bucket including the overflow slot",
      snap.count
    );
    assert_eq!(
      snap.cumulative.len(),
      BUCKETS.len() + 1,
      "one sample line per bound, plus +Inf"
    );
  }

  /// Buckets are cumulative in the Prometheus format - emitting raw
  /// per-bucket hits is a silent, plausible-looking wrong answer.
  #[test]
  fn buckets_are_cumulative_and_never_decrease() {
    observe(GitOp::Clone, true, Duration::from_millis(50));
    observe(GitOp::Clone, true, Duration::from_secs(20));

    let snap = snapshot()
      .into_iter()
      .find(|s| matches!(s.op, GitOp::Clone))
      .expect("clone is one of GitOp::ALL");

    let mut previous = 0;
    for (label, value) in &snap.cumulative {
      assert!(
        *value >= previous,
        "bucket {label} went backwards ({value} < {previous}), so \
         these are not cumulative"
      );
      previous = *value;
    }
    assert!(previous >= 2, "both observations should be counted");
  }

  /// A failed fetch still contributes its duration; a timeout is
  /// precisely the latency an operator needs to see.
  #[test]
  fn failures_are_counted_and_still_timed() {
    observe(GitOp::Fetch, false, Duration::from_secs(2));

    let snap = snapshot()
      .into_iter()
      .find(|s| matches!(s.op, GitOp::Fetch))
      .expect("fetch is one of GitOp::ALL");

    assert!(snap.failures >= 1, "the failure was not counted");
    assert!(
      snap.sum_seconds >= 2.0,
      "a failed operation's duration must still be in the sum, got {}",
      snap.sum_seconds
    );
  }
}
