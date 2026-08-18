# Grafana dashboard for Komodo

`komodo-overview.json` is an example dashboard for Core's `/metrics`
endpoint. Import it via **Dashboards → New → Import**, then pick your
Prometheus datasource when prompted — the dashboard takes it as a
variable rather than hard-coding a uid, so it works on any install.

## Scraping Core

`/metrics` is unauthenticated, like `/version`, because a Prometheus
scraper cannot present a user session:

```yaml
scrape_configs:
  - job_name: komodo
    metrics_path: /metrics
    static_configs:
      - targets: ["komodo.example.com"]
```

## What is exposed

| Metric | Type | Labels |
|---|---|---|
| `komodo_servers` | gauge | `state` |
| `komodo_clusters` | gauge | `state` |
| `komodo_stacks` | gauge | `state` |
| `komodo_deployments` | gauge | `state` |
| `komodo_swarms` | gauge | `state` |
| `komodo_executions_in_flight` | gauge | `resource` |
| `komodo_git_remote_duration_seconds` | histogram | `operation` |
| `komodo_git_remote_failures_total` | counter | `operation` |

The gauges are derived from the status caches Core's poll loops already
maintain, not from new bookkeeping, so a scrape cannot drift from what
the Komodo UI shows.

## What it deliberately does not tell you

**Counts, never names.** A scrape answers "how many Servers are
unreachable", never *which*. That keeps an unauthenticated endpoint from
disclosing your resource inventory, and it means alerting on these
metrics tells you to go look at Komodo — it is not a replacement for
looking.

**Git timings are Core's own.** `lib/git` is linked by both Core and
Periphery, and the counters are process-local. Core's fetching is the
repo-server-equivalent work — resource syncs, Stacks, Repos and Builds
reading remote config. Periphery's clones during a build are *not* in
these numbers; Periphery has no scrape endpoint to report them on.

**Only remote-contacting git commands are timed** — `clone`, `fetch`,
`pull`. Local steps in the same code path (checkout, reset, reading the
latest commit) are excluded on purpose: folding them in would move the
number for reasons unrelated to the remote, which is the one thing these
timings exist to diagnose. A cached pull — `pull` short-circuits within
its timeout without contacting anything — records nothing at all.

**Failed operations still contribute their duration.** A timeout is
precisely the latency worth seeing, so dropping failures would hide the
worst cases. Read the failure-rate panel next to the latency panel: both
spiking means a slow or hanging remote, failures alone means something
is being rejected outright.

**Buckets top out at 60s.** A p99 sitting at 60 means "at least 60s",
not exactly 60.

## Keeping the dashboard honest

A dashboard that names a metric Core does not emit does not fail — every
affected panel just renders `No data`, which looks like a quiet system.
`bin/core/src/api/mod.rs` has a test (`grafana_dashboard`) that extracts
every `komodo_*` name from this JSON and asserts Core actually emits it,
so a rename breaks the build instead of the dashboard.
