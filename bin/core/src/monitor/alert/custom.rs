//! Admin-authored alert conditions on a Server.
//!
//! The built-in cpu / memory / disk alerts are fixed code paths with
//! fixed thresholds. This evaluates expressions instead, so a new
//! condition is a config edit rather than a release.
//!
//! Point-in-time, not open/close: a condition fires once when it
//! becomes true and once more when it stops being true. The built-in
//! alerts keep an open Alert document keyed by their variant, and
//! several custom conditions on one Server would all collide on
//! `AlertData::Custom` in that scheme. The rising edge is what a
//! notification is for anyway.

use evalexpr::{
  ContextWithMutableVariables, DefaultNumericTypes, HashMapContext,
  Value, eval_boolean_with_context,
};
use komodo_client::entities::{
  ResourceTarget,
  alert::{Alert, AlertData, SeverityLevel},
  server::Server,
};

use crate::state::{CachedServerStatus, custom_alert_state_cache};

/// Evaluate every enabled condition on the Server and return the
/// alerts for the ones that just changed state.
pub async fn custom_server_alerts(
  server: &Server,
  status: &CachedServerStatus,
  ts: i64,
) -> Vec<Alert> {
  if server.config.custom_alerts.is_empty() {
    return Vec::new();
  }

  let context = context(status);
  let mut alerts = Vec::new();

  for custom in &server.config.custom_alerts {
    if !custom.enabled || custom.is_none() {
      continue;
    }
    let key = format!("{}:{}", server.id, custom.name);
    let firing = match eval_boolean_with_context(
      custom.expression.trim(),
      &context,
    ) {
      Ok(firing) => firing,
      Err(e) => {
        // A broken expression is reported once per edge, not once per
        // monitoring cycle, or a typo becomes a log flood.
        if custom_alert_state_cache().get(&key).await != Some(false) {
          custom_alert_state_cache().insert(key, false).await;
          warn!(
            server = server.name,
            condition = custom.name,
            "custom alert expression failed to evaluate | {e}"
          );
        }
        continue;
      }
    };

    let previous = custom_alert_state_cache().get(&key).await;
    custom_alert_state_cache().insert(key, firing).await;

    // First sight of a condition that is not firing is not an event.
    // Without this every Core restart would announce every condition
    // it found already false.
    let Some(previous) = previous else {
      continue;
    };
    if previous == firing {
      continue;
    }

    alerts.push(Alert {
      id: Default::default(),
      ts,
      resolved: !firing,
      resolved_ts: (!firing).then_some(ts),
      level: if firing {
        custom.level
      } else {
        SeverityLevel::Ok
      },
      target: ResourceTarget::Server(server.id.clone()),
      data: AlertData::Custom {
        message: if firing {
          format!(
            "Server {} matched alert condition '{}'",
            server.name, custom.name
          )
        } else {
          format!(
            "Server {} no longer matches alert condition '{}'",
            server.name, custom.name
          )
        },
        details: custom.expression.trim().to_string(),
      },
    });
  }

  alerts
}

/// The variables an expression may name.
///
/// Absent stats mean the values are 0 rather than the expression
/// failing: a Server with stats monitoring off should not produce an
/// error every cycle. `state` is what distinguishes "quiet because
/// idle" from "quiet because unreachable".
fn context(
  status: &CachedServerStatus,
) -> HashMapContext<DefaultNumericTypes> {
  let mut context = HashMapContext::new();
  let mut set = |name: &str, value: Value<DefaultNumericTypes>| {
    // Only fails on a name that is not a valid identifier, and every
    // name here is a literal below.
    let _ = context.set_value(name.to_string(), value);
  };

  set("state", Value::from(format!("{:?}", status.state)));

  let percentage = |used: f64, total: f64| {
    if total > 0.0 {
      used / total * 100.0
    } else {
      0.0
    }
  };

  if let Some(stats) = &status.system_stats {
    set("cpu_perc", Value::from_float(stats.cpu_perc as f64));
    set("load_1", Value::from_float(stats.load_average.one));
    set("load_5", Value::from_float(stats.load_average.five));
    set("load_15", Value::from_float(stats.load_average.fifteen));
    set("mem_used_gb", Value::from_float(stats.mem_used_gb));
    set("mem_total_gb", Value::from_float(stats.mem_total_gb));
    set("mem_free_gb", Value::from_float(stats.mem_free_gb));
    set(
      "mem_buff_cache_gb",
      Value::from_float(stats.mem_buff_cache_gb),
    );
    set("mem_zfs_arc_gb", Value::from_float(stats.mem_zfs_arc_gb));
    set(
      "mem_perc",
      Value::from_float(percentage(
        stats.mem_used_gb,
        stats.mem_total_gb,
      )),
    );
    set("swap_used_gb", Value::from_float(stats.swap_used_gb));
    set("swap_total_gb", Value::from_float(stats.swap_total_gb));
    set(
      "swap_perc",
      Value::from_float(percentage(
        stats.swap_used_gb,
        stats.swap_total_gb,
      )),
    );
    set(
      "network_ingress_bytes",
      Value::from_float(stats.network_ingress_bytes),
    );
    set(
      "network_egress_bytes",
      Value::from_float(stats.network_egress_bytes),
    );

    // The fullest disk, because "the disk is filling up" is about
    // whichever one runs out first, not about the average.
    let fullest = stats
      .disks
      .iter()
      .max_by(|a, b| {
        percentage(a.used_gb, a.total_gb)
          .total_cmp(&percentage(b.used_gb, b.total_gb))
      })
      .map(|disk| (disk.used_gb, disk.total_gb))
      .unwrap_or((0.0, 0.0));
    set("disk_used_gb", Value::from_float(fullest.0));
    set("disk_total_gb", Value::from_float(fullest.1));
    set(
      "disk_perc",
      Value::from_float(percentage(fullest.0, fullest.1)),
    );
  } else {
    for name in [
      "cpu_perc",
      "load_1",
      "load_5",
      "load_15",
      "mem_used_gb",
      "mem_total_gb",
      "mem_free_gb",
      "mem_buff_cache_gb",
      "mem_zfs_arc_gb",
      "mem_perc",
      "swap_used_gb",
      "swap_total_gb",
      "swap_perc",
      "network_ingress_bytes",
      "network_egress_bytes",
      "disk_used_gb",
      "disk_total_gb",
      "disk_perc",
    ] {
      set(name, Value::from_float(0.0));
    }
  }

  let (containers, running) = status
    .docker
    .as_ref()
    .map(|docker| {
      let running = docker
        .containers
        .iter()
        .filter(|container| {
          matches!(
            container.state,
            komodo_client::entities::docker::container::ContainerStateStatusEnum::Running
          )
        })
        .count();
      (docker.containers.len(), running)
    })
    .unwrap_or((0, 0));
  set("containers", Value::from_int(containers as i64));
  set("containers_running", Value::from_int(running as i64));

  context
}

#[cfg(test)]
mod tests {
  use komodo_client::entities::{
    server::ServerState,
    stats::{SingleDiskUsage, SystemStats},
  };

  use super::*;

  fn status() -> CachedServerStatus {
    CachedServerStatus {
      state: ServerState::Ok,
      system_stats: Some(SystemStats {
        cpu_perc: 42.0,
        mem_used_gb: 8.0,
        mem_total_gb: 16.0,
        swap_used_gb: 2.0,
        swap_total_gb: 4.0,
        disks: vec![
          SingleDiskUsage {
            mount: "/".into(),
            file_system: String::from("ext4"),
            used_gb: 10.0,
            total_gb: 100.0,
          },
          SingleDiskUsage {
            mount: "/data".into(),
            file_system: String::from("ext4"),
            used_gb: 90.0,
            total_gb: 100.0,
          },
        ],
        ..Default::default()
      }),
      ..Default::default()
    }
  }

  fn eval(expression: &str) -> bool {
    eval_boolean_with_context(expression, &context(&status()))
      .unwrap()
  }

  #[test]
  fn percentages_are_derived_not_read() {
    assert!(eval("mem_perc == 50.0"));
    assert!(eval("swap_perc == 50.0"));
    // The comparison authors will actually write: an int literal
    // against a float value has to work, or every threshold needs a
    // decimal point nobody would think to add.
    assert!(eval("mem_perc > 40"));
    assert!(eval("mem_perc >= 50"));
    assert!(!eval("mem_perc > 60"));
  }

  #[test]
  fn disk_variables_describe_the_fullest_disk() {
    // Not the first disk, and not the average: "the disk is filling
    // up" is about whichever one runs out first.
    assert!(eval("disk_perc == 90.0"));
    assert!(eval("disk_used_gb > 89"));
  }

  #[test]
  fn conditions_can_combine_and_read_state() {
    assert!(eval("cpu_perc > 40 && mem_perc >= 50"));
    assert!(eval("state == \"Ok\""));
    assert!(!eval("state != \"Ok\""));
  }

  #[test]
  fn missing_stats_are_zero_rather_than_an_error() {
    let status = CachedServerStatus {
      state: ServerState::NotOk,
      ..Default::default()
    };
    // A Server with stats monitoring off must not make every
    // condition on it fail to evaluate.
    assert!(
      !eval_boolean_with_context("cpu_perc > 90", &context(&status))
        .unwrap()
    );
    assert!(
      eval_boolean_with_context(
        "state == \"NotOk\"",
        &context(&status)
      )
      .unwrap()
    );
  }
}
