use std::{collections::HashMap, path::PathBuf};

use bson::Document;
use derive_builder::Builder;
use partial_derive2::Partial;
use serde::{Deserialize, Serialize};
use strum::Display;
use typeshare::typeshare;

use crate::{
  deserializers::{
    option_string_list_deserializer, string_list_deserializer,
  },
  entities::{
    _Serror, MaintenanceWindow, Timelength, stats::MinimalSystemStats,
  },
};

use super::{
  alert::SeverityLevel,
  resource::{AddFilters, Resource, ResourceListItem, ResourceQuery},
};

#[cfg(feature = "utoipa")]
#[derive(utoipa::ToSchema)]
#[schema(as = Server)]
pub struct ServerSchema(
  #[schema(inline)] pub Resource<ServerConfig, ServerInfo>,
);

#[typeshare]
pub type Server = Resource<ServerConfig, ServerInfo>;

#[typeshare]
pub type ServerListItem = ResourceListItem<ServerListItemInfo>;

#[typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerListItemInfo {
  /// The server's state.
  pub state: ServerState,
  /// If there is an error reaching
  /// the server, message will be given here.
  pub err: Option<_Serror>,
  /// System stats, if available
  pub stats: Option<MinimalSystemStats>,
  /// The server alerting thresholds
  pub alerting_thresholds: ServerAlertingThresholds,
  /// The server's number of physical cores.
  pub core_count: Option<u32>,
  /// The server's number of logical cores.
  pub logical_core_count: Option<u32>,
  /// Region of the server.
  pub region: String,
  /// The Cluster this Server is a node of, or null if it is not one.
  pub cluster_id: Option<String>,
  /// What Kubernetes calls this node, or null if it is not a Cluster
  /// node. Falls back to the Server's own name when unset.
  pub node_name: Option<String>,
  /// Address of the server, or null if empty.
  pub address: Option<String>,
  /// External address of the server (reachable by users).
  /// Used with links.
  pub external_address: Option<String>,
  /// Host public ip, if it could be resolved.
  pub public_ip: Option<String>,
  /// Whether server is configured to send disconnected alerts.
  pub send_unreachable_alerts: bool,
  /// Whether server is configured to send cpu alerts.
  pub send_cpu_alerts: bool,
  /// Whether server is configured to send mem alerts.
  pub send_mem_alerts: bool,
  /// Whether server is configured to send disk alerts.
  pub send_disk_alerts: bool,
  /// Whether server is configured to send version mismatch alerts.
  pub send_version_mismatch_alerts: bool,
  /// The Komodo Periphery version.
  pub version: Option<String>,
  /// The public key of Periphery
  pub public_key: Option<String>,
  /// If a Periphery fails to authenticate to Core with invalid Periphery public key,
  /// it will be stored here to accept the connection later on.
  pub attempted_public_key: Option<String>,
  /// Whether server is configured to send unreachable alerts.
  /// Whether terminals are disabled for this Server.
  pub terminals_disabled: bool,
  /// Whether container terminals are disabled for this Server.
  pub container_terminals_disabled: bool,
}

#[typeshare]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerInfo {
  /// If a Periphery fails to authenticate to Core
  /// for a disconnected server with invalid Periphery public key,
  /// it will be stored here to accept the connection later on.
  #[serde(default)]
  pub attempted_public_key: String,
  /// The expected public key associated with
  /// private key of the periphery agent.
  #[serde(default)]
  pub public_key: String,
}

#[typeshare(serialized_as = "Partial<ServerConfig>")]
pub type _PartialServerConfig = PartialServerConfig;

/// Server configuration.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Builder, Partial)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[partial_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(
  feature = "schemars",
  partial_derive(schemars::JsonSchema)
)]
#[diff_derive(Serialize, Deserialize, Debug, Clone, Default)]
#[partial(skip_serializing_none, from, diff)]
pub struct ServerConfig {
  /// The ws/s address of the periphery client.
  /// If unset, Server expects Periphery -> Core connection.
  #[serde(default)]
  #[builder(default)]
  pub address: String,

  /// Only relevant for Core -> Periphery connections.
  /// Whether to skip Periphery tls certificate validation.
  /// This defaults to true because Periphery generates self-signed certificates by default,
  /// but if you use valid certs you can switch this to false.
  #[serde(default = "default_insecure_tls")]
  #[builder(default = "default_insecure_tls()")]
  #[partial_default(default_insecure_tls())]
  pub insecure_tls: bool,

  /// The address to use with links for containers on the server.
  /// If empty, will use the 'address' for links.
  #[serde(default)]
  #[builder(default)]
  pub external_address: String,

  /// An optional region label
  #[serde(default)]
  #[builder(default)]
  pub region: String,

  /// The Cluster this Server is a Kubernetes node of, if any.
  ///
  /// Distinct from [ClusterConfig::server_id][crate::entities::cluster::ClusterConfig],
  /// which names the one Server whose Periphery runs kubectl for a
  /// Cluster. This is the reverse relation: it marks a Server as a node
  /// *inside* a Cluster, and a Cluster's control host need not be one of
  /// its own nodes.
  #[serde(default, alias = "cluster")]
  #[partial_attr(serde(alias = "cluster"))]
  #[cfg_attr(
    feature = "schemars",
    partial_attr(schemars(rename = "cluster"))
  )]
  #[builder(default)]
  pub cluster_id: String,

  /// What Kubernetes calls this node (`kubectl get nodes`).
  ///
  /// Only meaningful with [ServerConfig::cluster_id] set. Empty means the
  /// node carries the Server's own name, which is the common case - set
  /// it when the two genuinely differ.
  #[serde(default)]
  #[builder(default)]
  pub node_name: String,

  /// Whether a server is enabled.
  /// If a server is disabled,
  /// you won't be able to perform any actions on it or see deployment's status.
  /// Default: false
  #[serde(default = "default_enabled")]
  #[builder(default = "default_enabled()")]
  #[partial_default(default_enabled())]
  pub enabled: bool,

  /// Whether to automatically rotate Server keys when
  /// RotateAllServerKeys is called.
  /// Default: true
  #[serde(default = "default_auto_rotate_keys")]
  #[builder(default = "default_auto_rotate_keys()")]
  #[partial_default(default_auto_rotate_keys())]
  pub auto_rotate_keys: bool,

  /// Deprecated. Use private / public keys instead.
  /// An optional override passkey to use
  /// to authenticate with periphery agent.
  /// If this is empty, will use passkey in core config.
  #[serde(default)]
  #[builder(default)]
  pub passkey: String,

  /// Sometimes the system stats reports a mount path that is not desired.
  /// Use this field to filter it out from the report.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub ignore_mounts: Vec<String>,

  /// Object names matched here are never reported as orphaned.
  /// Supports wildcards, or a regex when wrapped in backslashes.
  /// For containers and compose projects deliberately run by hand.
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub ignore_orphans: Vec<String>,

  /// Refuse a deploy that would take over a container stamped as
  /// belonging to a different Komodo resource.
  ///
  /// Off by default, because it is not always wrong: a Deployment
  /// deleted and recreated under the same name legitimately meets its
  /// predecessor's container, and refusing that would be a
  /// regression. Turn it on for hosts shared between resources, where
  /// silently adopting someone else's container is the worse outcome.
  #[serde(default)]
  #[builder(default)]
  pub fail_on_shared_containers: bool,

  /// Alert conditions written as expressions, evaluated against this
  /// Server's live stats every monitoring cycle.
  ///
  /// The escape hatch from "every new alert condition is a code
  /// change": the built-in cpu / memory / disk thresholds cover the
  /// common cases, and this covers the rest.
  #[serde(default)]
  #[builder(default)]
  pub custom_alerts: Vec<CustomAlert>,

  /// Whether to trigger 'docker image prune -a -f' every 24 hours.
  /// default: true
  #[serde(default = "default_auto_prune")]
  #[builder(default = "default_auto_prune()")]
  #[partial_default(default_auto_prune())]
  pub auto_prune: bool,

  /// Configure quick links that are displayed in the resource header
  #[serde(default, deserialize_with = "string_list_deserializer")]
  #[partial_attr(serde(
    default,
    deserialize_with = "option_string_list_deserializer"
  ))]
  #[builder(default)]
  pub links: Vec<String>,

  /// Whether to monitor any server stats beyond passing health check.
  /// default: true
  #[serde(default = "default_stats_monitoring")]
  #[builder(default = "default_stats_monitoring()")]
  #[partial_default(default_stats_monitoring())]
  pub stats_monitoring: bool,

  /// Whether to send alerts about the servers reachability
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_unreachable_alerts: bool,

  /// Whether to send alerts about the servers CPU status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_cpu_alerts: bool,

  /// Whether to send alerts about the servers MEM status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_mem_alerts: bool,

  /// Whether to send alerts about the servers DISK status
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_disk_alerts: bool,

  /// Whether to send alerts about the servers version mismatch with core
  #[serde(default = "default_send_alerts")]
  #[builder(default = "default_send_alerts()")]
  #[partial_default(default_send_alerts())]
  pub send_version_mismatch_alerts: bool,

  /// The percentage threshhold which triggers WARNING state for CPU.
  #[serde(default = "default_cpu_warning")]
  #[builder(default = "default_cpu_warning()")]
  #[partial_default(default_cpu_warning())]
  pub cpu_warning: f32,

  /// The percentage threshhold which triggers CRITICAL state for CPU.
  #[serde(default = "default_cpu_critical")]
  #[builder(default = "default_cpu_critical()")]
  #[partial_default(default_cpu_critical())]
  pub cpu_critical: f32,

  /// The percentage threshhold which triggers WARNING state for MEM.
  #[serde(default = "default_mem_warning")]
  #[builder(default = "default_mem_warning()")]
  #[partial_default(default_mem_warning())]
  pub mem_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for MEM.
  #[serde(default = "default_mem_critical")]
  #[builder(default = "default_mem_critical()")]
  #[partial_default(default_mem_critical())]
  pub mem_critical: f64,

  /// The percentage threshhold which triggers WARNING state for DISK.
  #[serde(default = "default_disk_warning")]
  #[builder(default = "default_disk_warning()")]
  #[partial_default(default_disk_warning())]
  pub disk_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for DISK.
  #[serde(default = "default_disk_critical")]
  #[builder(default = "default_disk_critical()")]
  #[partial_default(default_disk_critical())]
  pub disk_critical: f64,

  /// Scheduled maintenance windows during which alerts will be suppressed.
  #[serde(default)]
  #[builder(default)]
  pub maintenance_windows: Vec<MaintenanceWindow>,
}

impl ServerConfig {
  pub fn builder() -> ServerConfigBuilder {
    ServerConfigBuilder::default()
  }
}

fn default_insecure_tls() -> bool {
  // Peripheries use self signed certs by default
  true
}

fn default_enabled() -> bool {
  false
}

fn default_auto_rotate_keys() -> bool {
  true
}

fn default_stats_monitoring() -> bool {
  true
}

fn default_auto_prune() -> bool {
  true
}

fn default_send_alerts() -> bool {
  true
}

fn default_cpu_warning() -> f32 {
  90.0
}

fn default_cpu_critical() -> f32 {
  99.0
}

fn default_mem_warning() -> f64 {
  75.0
}

fn default_mem_critical() -> f64 {
  95.0
}

fn default_disk_warning() -> f64 {
  75.0
}

fn default_disk_critical() -> f64 {
  95.0
}

impl Default for ServerConfig {
  fn default() -> Self {
    Self {
      address: Default::default(),
      insecure_tls: default_insecure_tls(),
      external_address: Default::default(),
      cluster_id: Default::default(),
      node_name: Default::default(),
      enabled: default_enabled(),
      auto_rotate_keys: default_auto_rotate_keys(),
      ignore_mounts: Default::default(),
      ignore_orphans: Default::default(),
      fail_on_shared_containers: Default::default(),
      custom_alerts: Default::default(),
      stats_monitoring: default_stats_monitoring(),
      auto_prune: default_auto_prune(),
      links: Default::default(),
      send_unreachable_alerts: default_send_alerts(),
      send_cpu_alerts: default_send_alerts(),
      send_mem_alerts: default_send_alerts(),
      send_disk_alerts: default_send_alerts(),
      send_version_mismatch_alerts: default_send_alerts(),
      region: Default::default(),
      passkey: Default::default(),
      cpu_warning: default_cpu_warning(),
      cpu_critical: default_cpu_critical(),
      mem_warning: default_mem_warning(),
      mem_critical: default_mem_critical(),
      disk_warning: default_disk_warning(),
      disk_critical: default_disk_critical(),
      maintenance_windows: Default::default(),
    }
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::PartialSchema for PartialServerConfig {
  fn schema()
  -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::schema!(#[inline] std::collections::HashMap<String, serde_json::Value>).into()
  }
}

#[cfg(feature = "utoipa")]
impl utoipa::ToSchema for PartialServerConfig {}

/// Just the server alerting thresholds
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerAlertingThresholds {
  /// The percentage threshhold which triggers WARNING state for CPU.
  #[serde(default = "default_cpu_warning")]
  pub cpu_warning: f32,

  /// The percentage threshhold which triggers CRITICAL state for CPU.
  #[serde(default = "default_cpu_critical")]
  pub cpu_critical: f32,

  /// The percentage threshhold which triggers WARNING state for MEM.
  #[serde(default = "default_mem_warning")]
  pub mem_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for MEM.
  #[serde(default = "default_mem_critical")]
  pub mem_critical: f64,

  /// The percentage threshhold which triggers WARNING state for DISK.
  #[serde(default = "default_disk_warning")]
  pub disk_warning: f64,

  /// The percentage threshhold which triggers CRITICAL state for DISK.
  #[serde(default = "default_disk_critical")]
  pub disk_critical: f64,
}

impl From<&ServerConfig> for ServerAlertingThresholds {
  fn from(config: &ServerConfig) -> Self {
    ServerAlertingThresholds {
      cpu_warning: config.cpu_warning,
      cpu_critical: config.cpu_critical,
      mem_warning: config.mem_warning,
      mem_critical: config.mem_critical,
      disk_warning: config.disk_warning,
      disk_critical: config.disk_critical,
    }
  }
}

/// The health of a part of the server.
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerHealthState {
  pub level: SeverityLevel,
  /// Whether the health is good enough to close an open alert.
  pub should_close_alert: bool,
}

/// Summary of the health of the server.
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerHealth {
  pub cpu: ServerHealthState,
  pub mem: ServerHealthState,
  #[cfg_attr(feature = "utoipa", schema(value_type = HashMap<String, ServerHealthState>))]
  pub disks: HashMap<PathBuf, ServerHealthState>,
}

/// Info about Periphery configuration
#[typeshare]
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct PeripheryInformation {
  /// The Periphery version.
  pub version: String,
  /// The public key of Periphery
  pub public_key: String,
  /// Whether terminals are disabled on this Periphery server
  pub terminals_disabled: bool,
  /// Whether container exec is disabled on this Periphery server
  pub container_terminals_disabled: bool,
  /// The rate the system stats are being polled from the system
  pub stats_polling_rate: Timelength,
  /// Whether Periphery is successfully connected to docker daemon.
  pub docker_connected: bool,
  /// The host public ip, if it can be resolved.
  pub public_ip: Option<String>,
  /// The features this Periphery build understands, from
  /// [periphery_capability]. Empty from any agent predating the
  /// handshake, which is exactly the agent Core must refuse - hence
  /// `default` rather than a hard deserialize error.
  #[serde(default)]
  pub capabilities: Vec<String>,
}

/// Feature names Periphery reports in [PeripheryInformation::capabilities].
///
/// A new *response* shape can be made backwards readable with an
/// untagged enum, but a new *request field* is simply dropped by an
/// older agent, which then does the wrong thing without erroring. The
/// version string cannot tell them apart - a fork build and upstream
/// both report the workspace version. So each request field whose
/// absence changes behaviour gets a name here, Periphery reports the
/// ones its build has, and Core refuses when the field is set and the
/// name is missing.
pub mod periphery_capability {
  /// `ApplyClusterManifests.helm` is rendered with `helm template`
  /// before applying. An agent without it applies the raw source.
  pub const CLUSTER_HELM_RENDER: &str = "cluster_helm_render";

  /// `ApplyClusterManifests.mode` is honoured. The field defaults to
  /// Apply, so an agent without it turns a Diff or a Delete into an
  /// apply - the worst of the three, since Diff is the mode users
  /// reach for precisely because it touches nothing.
  pub const CLUSTER_APPLY_MODE: &str = "cluster_apply_mode";

  /// `ApplyClusterManifests.wait_ready` runs `kubectl rollout status`.
  /// An agent without it reports success the moment kubectl accepts
  /// the objects, whether or not they ever become ready.
  pub const CLUSTER_WAIT_READY: &str = "cluster_wait_ready";

  /// `ApplyClusterManifests.policy` is enforced against the objects
  /// kubectl is about to send. An agent without it applies with no
  /// blast-radius controls at all - and Core's own scan of the
  /// declared text cannot stand in for it wherever the declared text
  /// is not what reaches the cluster.
  pub const CLUSTER_MANIFEST_POLICY: &str = "cluster_manifest_policy";

  /// `GetClusterResources` honours `label_selector` / `field_selector` /
  /// `limit` / `summary`. An agent without it ignores the filters and
  /// returns every object in full, which the caller would mistake for
  /// the filtered set.
  pub const CLUSTER_RESOURCE_FILTERS: &str =
    "cluster_resource_filters";

  /// `GetClusterPodLog` honours `label_selector` / `all_containers` /
  /// `since` / `since_time`. An agent without it ignores them and
  /// returns the wrong log window.
  pub const CLUSTER_LOG_OPTIONS: &str = "cluster_log_options";

  /// `ApplyClusterObject.mode` is honoured. An agent without it turns
  /// a DryRun or a Diff into a real apply - the exact opposite of what
  /// those modes are for.
  pub const CLUSTER_OBJECT_MODE: &str = "cluster_object_mode";

  /// Everything this build supports. Periphery reports it verbatim, so
  /// adding a const above and listing it here is the whole change.
  pub const ALL: &[&str] = &[
    CLUSTER_HELM_RENDER,
    CLUSTER_APPLY_MODE,
    CLUSTER_WAIT_READY,
    CLUSTER_MANIFEST_POLICY,
    CLUSTER_RESOURCE_FILTERS,
    CLUSTER_LOG_OPTIONS,
    CLUSTER_OBJECT_MODE,
  ];
}

/// Current pending actions on the server.
#[typeshare]
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerActionState {
  /// Server currently pruning networks
  pub pruning_networks: bool,
  /// Server currently pruning containers
  pub pruning_containers: bool,
  /// Server currently pruning images
  pub pruning_images: bool,
  /// Server currently pruning volumes
  pub pruning_volumes: bool,
  /// Server currently pruning docker builders
  pub pruning_builders: bool,
  /// Server currently pruning builx cache
  pub pruning_buildx: bool,
  /// Server currently pruning system
  pub pruning_system: bool,
  /// Server currently starting containers.
  pub starting_containers: u32,
  /// Server currently restarting containers.
  pub restarting_containers: u32,
  /// Server currently pausing containers.
  pub pausing_containers: u32,
  /// Server currently unpausing containers.
  pub unpausing_containers: u32,
  /// Server currently stopping containers.
  pub stopping_containers: u32,
  /// Server currently destroying containers.
  pub destroying_containers: u32,
}

#[typeshare]
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Hash,
  PartialOrd,
  Ord,
  Default,
  Display,
  Serialize,
  Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[strum(serialize_all = "kebab-case")]
pub enum ServerState {
  /// Server health check passing.
  Ok,
  /// Server is unreachable.
  #[default]
  NotOk,
  /// Server is disabled.
  Disabled,
}

/// Server-specific query
#[typeshare]
pub type ServerQuery = ResourceQuery<ServerQuerySpecifics>;

#[typeshare]
#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub enum ServerSortBy {
  /// Sort by name. Default.
  #[default]
  Name,
  /// Sort by region.
  Region,
  /// Sort by periphery version.
  Version,
  /// Sort by state.
  State,
  /// Sort by current cpu usage percentage.
  Cpu,
  /// Sort by current memory usage percentage.
  Memory,
  /// Sort by current disk usage percentage.
  Disk,
  /// Sort by current 1m load average.
  LoadAverage,
  /// Sort by current network usage (ingress + egress).
  Network,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ServerQuerySpecifics {
  /// Query only for Servers matching these states.
  /// If empty, does not filter by state.
  #[serde(default)]
  pub states: Vec<ServerState>,
  /// Query only for Servers which are nodes of these Clusters.
  /// If empty, does not filter by Cluster.
  #[serde(default)]
  pub clusters: Vec<String>,
}

impl AddFilters for ServerQuerySpecifics {
  fn add_filters(&self, filters: &mut Document) {
    if !self.clusters.is_empty() {
      filters.insert(
        "config.cluster_id",
        bson::doc! { "$in": &self.clusters },
      );
    }
  }
}

/// An admin-authored alert condition on a Server.
///
/// The expression is evaluated against the Server's live stats every
/// monitoring cycle and must produce a boolean. Available variables:
///
/// - `cpu_perc`, `load_1`, `load_5`, `load_15`
/// - `mem_used_gb`, `mem_total_gb`, `mem_perc`, `mem_free_gb`,
///   `mem_buff_cache_gb`, `mem_zfs_arc_gb`
/// - `swap_used_gb`, `swap_total_gb`, `swap_perc`
/// - `disk_used_gb`, `disk_total_gb`, `disk_perc` (the fullest disk)
/// - `network_ingress_bytes`, `network_egress_bytes`
/// - `containers`, `containers_running`
/// - `state` ("Ok" / "NotOk" / "Disabled")
///
/// For example: `mem_perc > 80 && swap_used_gb > 1`, or
/// `containers > 0 && containers_running == 0`.
///
/// Note. `<`, `>`, `<=` and `>=` compare a plain number against these
/// values as expected. `==` does not: the numeric values are floats,
/// so an exact comparison needs a decimal point (`mem_perc == 50.0`).
/// Counts (`containers`, `containers_running`) are integers and
/// compare exactly.
#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CustomAlert {
  /// Names the condition, and appears in the notification. Also the
  /// identity Komodo uses to tell "still true" from "just became
  /// true", so renaming one re-arms it.
  pub name: String,
  /// The condition. Must evaluate to a boolean.
  #[serde(default)]
  pub expression: String,
  /// The severity reported when it fires. Default: Warning
  #[serde(default = "default_custom_alert_level")]
  pub level: crate::entities::alert::SeverityLevel,
  /// Whether this condition is evaluated at all.
  #[serde(default = "default_enabled")]
  pub enabled: bool,
}

fn default_custom_alert_level()
-> crate::entities::alert::SeverityLevel {
  crate::entities::alert::SeverityLevel::Warning
}

impl CustomAlert {
  pub fn is_none(&self) -> bool {
    self.expression.trim().is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The whole handshake rests on this: an agent predating the
  /// capability field must still deserialize, and must land on the
  /// empty list rather than anything Core could read as support.
  #[test]
  fn old_periphery_reports_no_capabilities() {
    let info: PeripheryInformation = serde_json::from_str(
      r#"{
        "version": "2.3.1",
        "public_key": "abc",
        "terminals_disabled": false,
        "container_terminals_disabled": false,
        "stats_polling_rate": "5-sec",
        "docker_connected": true,
        "public_ip": null
      }"#,
    )
    .expect(
      "pre-handshake Periphery response must still deserialize",
    );
    assert!(info.capabilities.is_empty());
  }

  /// An empty ALL would make every guarded request refuse against a
  /// current agent, which is the failure mode nobody would notice
  /// until a helm deploy is blocked.
  #[test]
  fn current_periphery_reports_its_capabilities() {
    for cap in [
      periphery_capability::CLUSTER_HELM_RENDER,
      periphery_capability::CLUSTER_APPLY_MODE,
      periphery_capability::CLUSTER_WAIT_READY,
      periphery_capability::CLUSTER_MANIFEST_POLICY,
      periphery_capability::CLUSTER_RESOURCE_FILTERS,
      periphery_capability::CLUSTER_LOG_OPTIONS,
      periphery_capability::CLUSTER_OBJECT_MODE,
    ] {
      assert!(
        periphery_capability::ALL.contains(&cap),
        "{cap} is guarded on Core but not reported by Periphery"
      );
    }
  }
}
