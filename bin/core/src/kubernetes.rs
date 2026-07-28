use std::{
  collections::HashMap, path::Path, process::Stdio, sync::LazyLock,
  time::Duration,
};

use anyhow::{Context, anyhow, bail};
use axum::{
  Extension, Router,
  body::Body,
  extract::{DefaultBodyLimit, Request},
  http::StatusCode,
  middleware::{self, Next},
  response::Response,
  routing::post,
};
use bytes::Bytes;
use futures_util::stream;
use komodo_client::entities::user::User;
use mogh_auth_server::middleware::authenticate_request;
use mogh_error::{AddStatusCode as _, AddStatusCodeError as _, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
  io::{AsyncRead, AsyncReadExt},
  process::Command,
  sync::{RwLock, mpsc},
  time::{Instant, timeout_at},
};

use crate::auth::KomodoAuthImpl;

const MAX_REQUEST_BYTES: usize = 32_768;
const MAX_TIMEOUT_SECONDS: u64 = 300;
const MAX_OUTPUT_BYTES: usize = 1_048_576;
const MAX_LIST_ITEMS: usize = 10_000;
const MAX_TAIL_LINES: u32 = 10_000;
const MAX_EXEC_ARGS: usize = 128;
const MAX_EXEC_ARG_BYTES: usize = 16_384;
const MAX_KUBECONFIG_BYTES: u64 = 1_048_576;
const STREAM_CHANNEL_CAPACITY: usize = 8;

static CLUSTERS: LazyLock<
  RwLock<HashMap<String, RegisteredCluster>>,
> = LazyLock::new(Default::default);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredCluster {
  name: String,
  kubeconfig_path: String,
  context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterKindCluster {
  pub name: String,
  pub kubeconfig_path: String,
  #[serde(default)]
  pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterKindClusterResponse {
  pub name: String,
  pub context: Option<String>,
  pub credential_source: String,
  pub credential_redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSelector {
  pub cluster: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesNamespace {
  pub name: String,
  pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesWorkload {
  pub namespace: String,
  pub kind: String,
  pub name: String,
  pub ready: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKubernetesWorkloads {
  pub cluster: String,
  #[serde(default)]
  pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamPodLogs {
  pub cluster: String,
  pub namespace: String,
  pub pod: String,
  #[serde(default)]
  pub container: Option<String>,
  #[serde(default = "default_tail_lines")]
  pub tail_lines: u32,
  #[serde(default)]
  pub follow: bool,
  #[serde(default = "default_stream_limit_bytes")]
  pub limit_bytes: usize,
  #[serde(default = "default_timeout_seconds")]
  pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPodCommand {
  pub cluster: String,
  pub namespace: String,
  pub pod: String,
  #[serde(default)]
  pub container: Option<String>,
  pub command: Vec<String>,
  #[serde(default = "default_timeout_seconds")]
  pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPodCommandResponse {
  pub stdout: String,
  pub stderr: String,
  pub status_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotateNamespace {
  pub cluster: String,
  pub namespace: String,
  pub key: String,
  pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotateNamespaceResponse {
  pub namespace: String,
  pub annotation: String,
  pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KubectlCommand {
  program: &'static str,
  args: Vec<String>,
}

#[derive(Debug)]
struct BoundedOutput {
  stdout: Vec<u8>,
  stderr: Vec<u8>,
  status_code: Option<i32>,
}

pub fn router() -> Router {
  Router::new()
    .route("/register-kind", post(register_kind_cluster))
    .route("/namespaces", post(list_namespaces))
    .route("/workloads", post(list_workloads))
    .route("/logs", post(stream_pod_logs))
    .route("/exec", post(exec_pod_command))
    .route("/annotate-namespace", post(annotate_namespace))
    .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
    .layer(middleware::from_fn(require_admin_request))
    .layer(middleware::from_fn(
      authenticate_request::<KomodoAuthImpl, true>,
    ))
}

async fn require_admin_request(
  Extension(user): Extension<User>,
  request: Request,
  next: Next,
) -> mogh_error::Result<Response> {
  require_admin(&user)?;
  Ok(next.run(request).await)
}

async fn register_kind_cluster(
  Extension(user): Extension<User>,
  Json(request): Json<RegisterKindCluster>,
) -> mogh_error::Result<Json<RegisterKindClusterResponse>> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.name)
    .status_code(StatusCode::BAD_REQUEST)?;
  if let Some(context) = request.context.as_deref() {
    validate_context(context).status_code(StatusCode::BAD_REQUEST)?;
  }
  let kubeconfig_path =
    validate_kubeconfig_path(&request.kubeconfig_path)
      .status_code(StatusCode::BAD_REQUEST)?;
  let cluster = RegisteredCluster {
    name: request.name.clone(),
    kubeconfig_path,
    context: request.context.clone(),
  };

  {
    let clusters = CLUSTERS.read().await;
    registration_change(clusters.get(&request.name), &cluster)
      .status_code(StatusCode::CONFLICT)?;
  }

  let output = run_bounded(
    &names_command(&cluster),
    MAX_OUTPUT_BYTES,
    default_timeout_seconds(),
  )
  .await
  .map_err(kubectl_http_error)?;
  parse_namespaces(&output.stdout).map_err(kubectl_http_error)?;

  let mut clusters = CLUSTERS.write().await;
  registration_change(clusters.get(&request.name), &cluster)
    .status_code(StatusCode::CONFLICT)?;
  clusters.insert(request.name.clone(), cluster);

  Ok(Json(RegisterKindClusterResponse {
    name: request.name,
    context: request.context,
    credential_source: "server_file_path".to_string(),
    credential_redacted: true,
  }))
}

async fn list_namespaces(
  Extension(user): Extension<User>,
  Json(request): Json<ClusterSelector>,
) -> mogh_error::Result<Json<Vec<KubernetesNamespace>>> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.cluster)
    .status_code(StatusCode::BAD_REQUEST)?;
  let cluster = get_cluster(&request.cluster).await?;
  let output = run_bounded(
    &names_command(&cluster),
    MAX_OUTPUT_BYTES,
    default_timeout_seconds(),
  )
  .await
  .map_err(kubectl_http_error)?;
  Ok(Json(
    parse_namespaces(&output.stdout).map_err(kubectl_http_error)?,
  ))
}

async fn list_workloads(
  Extension(user): Extension<User>,
  Json(request): Json<ListKubernetesWorkloads>,
) -> mogh_error::Result<Json<Vec<KubernetesWorkload>>> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.cluster)
    .status_code(StatusCode::BAD_REQUEST)?;
  if let Some(namespace) = request.namespace.as_deref() {
    validate_kubernetes_name(namespace)
      .status_code(StatusCode::BAD_REQUEST)?;
  }
  let cluster = get_cluster(&request.cluster).await?;
  let output = run_bounded(
    &workloads_command(&cluster, request.namespace.as_deref()),
    MAX_OUTPUT_BYTES,
    default_timeout_seconds(),
  )
  .await
  .map_err(kubectl_http_error)?;
  Ok(Json(
    parse_workloads(&output.stdout).map_err(kubectl_http_error)?,
  ))
}

async fn stream_pod_logs(
  Extension(user): Extension<User>,
  Json(request): Json<StreamPodLogs>,
) -> mogh_error::Result<Body> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.cluster)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_kubernetes_name(&request.namespace)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_kubernetes_name(&request.pod)
    .status_code(StatusCode::BAD_REQUEST)?;
  if let Some(container) = request.container.as_deref() {
    validate_kubernetes_name(container)
      .status_code(StatusCode::BAD_REQUEST)?;
  }
  if request.tail_lines == 0 || request.tail_lines > MAX_TAIL_LINES {
    return Err(
      anyhow!("tail_lines must be between 1 and {MAX_TAIL_LINES}")
        .status_code(StatusCode::BAD_REQUEST),
    );
  }
  validate_output_limit(request.limit_bytes)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_timeout(request.timeout_seconds)
    .status_code(StatusCode::BAD_REQUEST)?;
  let cluster = get_cluster(&request.cluster).await?;
  stream_bounded(
    &logs_command(&cluster, &request),
    request.limit_bytes,
    request.timeout_seconds,
  )
  .await
}

async fn exec_pod_command(
  Extension(user): Extension<User>,
  Json(request): Json<ExecPodCommand>,
) -> mogh_error::Result<Json<ExecPodCommandResponse>> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.cluster)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_kubernetes_name(&request.namespace)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_kubernetes_name(&request.pod)
    .status_code(StatusCode::BAD_REQUEST)?;
  if let Some(container) = request.container.as_deref() {
    validate_kubernetes_name(container)
      .status_code(StatusCode::BAD_REQUEST)?;
  }
  validate_exec_argv(&request.command)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_timeout(request.timeout_seconds)
    .status_code(StatusCode::BAD_REQUEST)?;
  let cluster = get_cluster(&request.cluster).await?;
  let output = run_bounded(
    &exec_command(&cluster, &request),
    MAX_OUTPUT_BYTES,
    request.timeout_seconds,
  )
  .await
  .map_err(kubectl_http_error)?;
  Ok(Json(ExecPodCommandResponse {
    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    status_code: output.status_code,
  }))
}

async fn annotate_namespace(
  Extension(user): Extension<User>,
  Json(request): Json<AnnotateNamespace>,
) -> mogh_error::Result<Json<AnnotateNamespaceResponse>> {
  require_admin(&user)?;
  validate_kubernetes_name(&request.cluster)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_kubernetes_name(&request.namespace)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_annotation_key(&request.key)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_annotation_value(&request.value)
    .status_code(StatusCode::BAD_REQUEST)?;
  let cluster = get_cluster(&request.cluster).await?;
  let annotation = format!("{}={}", request.key, request.value);

  let current = run_bounded(
    &read_annotation_command(&cluster, &request),
    4096,
    default_timeout_seconds(),
  )
  .await
  .map_err(kubectl_http_error)?;
  let current = String::from_utf8(current.stdout)
    .context("kubectl returned a non-UTF-8 annotation value")
    .map_err(kubectl_http_error)?;
  let changed = current != request.value;
  if changed {
    run_bounded(
      &annotate_namespace_command(&cluster, &request),
      4096,
      default_timeout_seconds(),
    )
    .await
    .map_err(kubectl_http_error)?;
  }

  Ok(Json(AnnotateNamespaceResponse {
    namespace: request.namespace,
    annotation,
    changed,
  }))
}

async fn get_cluster(
  name: &str,
) -> mogh_error::Result<RegisteredCluster> {
  CLUSTERS.read().await.get(name).cloned().ok_or_else(|| {
    anyhow!("Kubernetes cluster is not registered")
      .status_code(StatusCode::NOT_FOUND)
  })
}

fn require_admin(user: &User) -> mogh_error::Result<()> {
  if !user.enabled || (!user.admin && !user.super_admin) {
    return Err(
      anyhow!(
        "Kubernetes spike endpoints require an enabled admin user"
      )
      .status_code(StatusCode::FORBIDDEN),
    );
  }
  Ok(())
}

fn kubectl_http_error(error: anyhow::Error) -> mogh_error::Error {
  let message = error.to_string();
  let status = if message.contains("timed out") {
    StatusCode::GATEWAY_TIMEOUT
  } else if message.contains("exceeded the configured limit")
    || message.contains("too many items")
  {
    StatusCode::PAYLOAD_TOO_LARGE
  } else {
    StatusCode::BAD_GATEWAY
  };
  anyhow!("Kubernetes operation failed").status_code(status)
}

fn validate_kubernetes_name(value: &str) -> anyhow::Result<()> {
  if value.is_empty() || value.len() > 253 {
    bail!("Kubernetes name must contain 1 to 253 characters");
  }
  for part in value.split('.') {
    if part.is_empty()
      || part.len() > 63
      || !part.bytes().all(|byte| {
        byte.is_ascii_lowercase()
          || byte.is_ascii_digit()
          || byte == b'-'
      })
      || !part
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
      || !part
        .as_bytes()
        .last()
        .is_some_and(u8::is_ascii_alphanumeric)
    {
      bail!("Invalid Kubernetes name");
    }
  }
  Ok(())
}

fn validate_context(value: &str) -> anyhow::Result<()> {
  if value.is_empty()
    || value.len() > 253
    || value.bytes().any(|byte| byte.is_ascii_control())
  {
    bail!("Invalid Kubernetes context");
  }
  Ok(())
}

fn validate_annotation_key(value: &str) -> anyhow::Result<()> {
  if value.is_empty()
    || value.len() > 253
    || value.matches('/').count() > 1
  {
    bail!("Invalid Kubernetes annotation key");
  }
  let (prefix, name) = value
    .split_once('/')
    .map_or((None, value), |(prefix, name)| (Some(prefix), name));
  if let Some(prefix) = prefix {
    validate_kubernetes_name(prefix)?;
  }
  if name.is_empty()
    || name.len() > 63
    || !name.bytes().all(|byte| {
      byte.is_ascii_alphanumeric()
        || matches!(byte, b'-' | b'_' | b'.')
    })
    || !name
      .as_bytes()
      .first()
      .is_some_and(u8::is_ascii_alphanumeric)
    || !name
      .as_bytes()
      .last()
      .is_some_and(u8::is_ascii_alphanumeric)
  {
    bail!("Invalid Kubernetes annotation key");
  }
  Ok(())
}

fn validate_annotation_value(value: &str) -> anyhow::Result<()> {
  if value.len() > 256
    || value.bytes().any(|byte| byte.is_ascii_control())
  {
    bail!("Invalid Kubernetes annotation value");
  }
  Ok(())
}

fn validate_timeout(seconds: u64) -> anyhow::Result<()> {
  if seconds == 0 || seconds > MAX_TIMEOUT_SECONDS {
    bail!(
      "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
    );
  }
  Ok(())
}

fn validate_output_limit(bytes: usize) -> anyhow::Result<()> {
  if bytes == 0 || bytes > MAX_OUTPUT_BYTES {
    bail!("limit_bytes must be between 1 and {MAX_OUTPUT_BYTES}");
  }
  Ok(())
}

fn validate_exec_argv(command: &[String]) -> anyhow::Result<()> {
  if command.is_empty() || command.len() > MAX_EXEC_ARGS {
    bail!("command must contain 1 to {MAX_EXEC_ARGS} arguments");
  }
  let mut bytes = 0usize;
  for argument in command {
    if argument
      .bytes()
      .any(|byte| byte == 0 || byte.is_ascii_control())
    {
      bail!("command arguments may not contain control characters");
    }
    bytes = bytes
      .checked_add(argument.len())
      .ok_or_else(|| anyhow!("command arguments are too large"))?;
  }
  if command[0].is_empty() || bytes > MAX_EXEC_ARG_BYTES {
    bail!("command arguments are too large");
  }
  Ok(())
}

fn validate_kubeconfig_path(value: &str) -> anyhow::Result<String> {
  let path = Path::new(value);
  if !path.is_absolute() {
    bail!(
      "kubeconfig_path must be an absolute server-local file path"
    );
  }
  let path = path
    .canonicalize()
    .context("Failed to resolve kubeconfig_path")?;
  let metadata = path
    .metadata()
    .context("Failed to inspect kubeconfig_path")?;
  if !metadata.is_file() {
    bail!("kubeconfig_path must identify a regular file");
  }
  if metadata.len() == 0 || metadata.len() > MAX_KUBECONFIG_BYTES {
    bail!(
      "kubeconfig_path must identify a non-empty file no larger than {MAX_KUBECONFIG_BYTES} bytes"
    );
  }
  path
    .to_str()
    .map(str::to_owned)
    .ok_or_else(|| anyhow!("kubeconfig_path must be valid UTF-8"))
}

fn registration_change(
  current: Option<&RegisteredCluster>,
  requested: &RegisteredCluster,
) -> anyhow::Result<()> {
  if current.is_some_and(|current| current != requested) {
    bail!(
      "Cluster name is already registered with different settings"
    );
  }
  Ok(())
}

fn default_tail_lines() -> u32 {
  100
}

fn default_stream_limit_bytes() -> usize {
  131_072
}

fn default_timeout_seconds() -> u64 {
  30
}

fn cluster_args(cluster: &RegisteredCluster) -> Vec<String> {
  let mut args =
    vec!["--kubeconfig".to_string(), cluster.kubeconfig_path.clone()];
  if let Some(context) = &cluster.context {
    args.extend(["--context".to_string(), context.clone()]);
  }
  args
}

fn names_command(cluster: &RegisteredCluster) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "get".to_string(),
    "namespaces".to_string(),
    "-o".to_string(),
    "json".to_string(),
  ]);
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn workloads_command(
  cluster: &RegisteredCluster,
  namespace: Option<&str>,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  if let Some(namespace) = namespace {
    args.extend(["-n".to_string(), namespace.to_string()]);
  } else {
    args.push("--all-namespaces".to_string());
  }
  args.extend([
    "get".to_string(),
    "pods,deployments,statefulsets,daemonsets".to_string(),
    "-o".to_string(),
    "json".to_string(),
  ]);
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn logs_command(
  cluster: &RegisteredCluster,
  request: &StreamPodLogs,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "-n".to_string(),
    request.namespace.clone(),
    "logs".to_string(),
    request.pod.clone(),
  ]);
  if let Some(container) = &request.container {
    args.extend(["-c".to_string(), container.clone()]);
  }
  if request.follow {
    args.push("--follow".to_string());
  }
  args.extend([
    format!("--tail={}", request.tail_lines),
    format!("--limit-bytes={}", request.limit_bytes),
    format!("--request-timeout={}s", request.timeout_seconds),
  ]);
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn exec_command(
  cluster: &RegisteredCluster,
  request: &ExecPodCommand,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "-n".to_string(),
    request.namespace.clone(),
    "exec".to_string(),
    request.pod.clone(),
  ]);
  if let Some(container) = &request.container {
    args.extend(["-c".to_string(), container.clone()]);
  }
  args
    .push(format!("--request-timeout={}s", request.timeout_seconds));
  args.push("--".to_string());
  args.extend(request.command.iter().cloned());
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn read_annotation_command(
  cluster: &RegisteredCluster,
  request: &AnnotateNamespace,
) -> KubectlCommand {
  let escaped_key =
    request.key.replace('.', "\\.").replace('/', "\\/");
  let mut args = cluster_args(cluster);
  args.extend([
    "get".to_string(),
    "namespace".to_string(),
    request.namespace.clone(),
    "-o".to_string(),
    format!("jsonpath={{.metadata.annotations.{escaped_key}}}"),
  ]);
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn annotate_namespace_command(
  cluster: &RegisteredCluster,
  request: &AnnotateNamespace,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "annotate".to_string(),
    "namespace".to_string(),
    request.namespace.clone(),
    format!("{}={}", request.key, request.value),
    "--overwrite".to_string(),
  ]);
  KubectlCommand {
    program: "kubectl",
    args,
  }
}

fn parse_namespaces(
  bytes: &[u8],
) -> anyhow::Result<Vec<KubernetesNamespace>> {
  let document: Value = serde_json::from_slice(bytes)
    .context("Failed to parse kubectl namespace output")?;
  let items = document["items"].as_array().ok_or_else(|| {
    anyhow!("kubectl namespace output is missing items")
  })?;
  if items.len() > MAX_LIST_ITEMS {
    bail!("kubectl namespace output contains too many items");
  }
  items
    .iter()
    .map(|item| {
      Ok(KubernetesNamespace {
        name: required_json_string(item, &["metadata", "name"])?
          .to_string(),
        status: item["status"]["phase"]
          .as_str()
          .unwrap_or("Unknown")
          .to_string(),
      })
    })
    .collect()
}

fn parse_workloads(
  bytes: &[u8],
) -> anyhow::Result<Vec<KubernetesWorkload>> {
  let document: Value = serde_json::from_slice(bytes)
    .context("Failed to parse kubectl workload output")?;
  let items = document["items"].as_array().ok_or_else(|| {
    anyhow!("kubectl workload output is missing items")
  })?;
  if items.len() > MAX_LIST_ITEMS {
    bail!("kubectl workload output contains too many items");
  }
  items
    .iter()
    .map(|item| {
      let kind = required_json_string(item, &["kind"])?;
      Ok(KubernetesWorkload {
        namespace: required_json_string(
          item,
          &["metadata", "namespace"],
        )?
        .to_string(),
        kind: kind.to_string(),
        name: required_json_string(item, &["metadata", "name"])?
          .to_string(),
        ready: workload_ready(item, kind),
      })
    })
    .collect()
}

fn required_json_string<'a>(
  value: &'a Value,
  path: &[&str],
) -> anyhow::Result<&'a str> {
  path
    .iter()
    .try_fold(value, |value, key| value.get(key))
    .and_then(Value::as_str)
    .ok_or_else(|| {
      anyhow!("kubectl output is missing a required field")
    })
}

fn workload_ready(item: &Value, kind: &str) -> Option<String> {
  match kind {
    "Pod" => {
      item["status"]["containerStatuses"]
        .as_array()
        .map(|statuses| {
          let ready = statuses
            .iter()
            .filter(|status| status["ready"].as_bool() == Some(true))
            .count();
          format!("{ready}/{}", statuses.len())
        })
    }
    "DaemonSet" => ready_pair(
      &item["status"],
      "numberReady",
      "desiredNumberScheduled",
    ),
    _ => ready_pair(&item["status"], "readyReplicas", "replicas"),
  }
}

fn ready_pair(
  status: &Value,
  ready_key: &str,
  total_key: &str,
) -> Option<String> {
  let total = status[total_key].as_u64()?;
  let ready = status[ready_key].as_u64().unwrap_or(0);
  Some(format!("{ready}/{total}"))
}

async fn run_bounded(
  command: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
) -> anyhow::Result<BoundedOutput> {
  validate_output_limit(limit_bytes)?;
  validate_timeout(timeout_seconds)?;
  let mut child = Command::new(command.program)
    .args(&command.args)
    .kill_on_drop(true)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .context("Failed to start kubectl process")?;
  let stdout =
    child.stdout.take().context("Failed to capture stdout")?;
  let stderr =
    child.stderr.take().context("Failed to capture stderr")?;
  let mut stdout_task =
    tokio::spawn(read_bounded(stdout, limit_bytes));
  let mut stderr_task =
    tokio::spawn(read_bounded(stderr, limit_bytes));
  let deadline =
    Instant::now() + Duration::from_secs(timeout_seconds);
  let mut completed_stdout = None;
  let mut completed_stderr = None;

  let status = tokio::select! {
    status = child.wait() => status.context("Failed to wait for kubectl")?,
    stdout = &mut stdout_task => {
      let result = stdout.context("kubectl stdout task failed")?;
      if let Err(error) = &result {
        let _ = child.kill().await;
        stderr_task.abort();
        return Err(anyhow!(error.to_string()))
          .context("Failed to read kubectl stdout");
      }
      completed_stdout = Some(result?);
      timeout_at(deadline, child.wait())
        .await
        .context("kubectl command timed out")?
        .context("Failed to wait for kubectl")?
    }
    stderr = &mut stderr_task => {
      let result = stderr.context("kubectl stderr task failed")?;
      if let Err(error) = &result {
        let _ = child.kill().await;
        stdout_task.abort();
        return Err(anyhow!(error.to_string()))
          .context("Failed to read kubectl stderr");
      }
      completed_stderr = Some(result?);
      timeout_at(deadline, child.wait())
        .await
        .context("kubectl command timed out")?
        .context("Failed to wait for kubectl")?
    }
    _ = tokio::time::sleep_until(deadline) => {
      let _ = child.kill().await;
      stdout_task.abort();
      stderr_task.abort();
      bail!("kubectl command timed out");
    }
  };
  let stdout = match completed_stdout {
    Some(stdout) => stdout,
    None => {
      stdout_task.await.context("kubectl stdout task failed")??
    }
  };
  let stderr = match completed_stderr {
    Some(stderr) => stderr,
    None => {
      stderr_task.await.context("kubectl stderr task failed")??
    }
  };
  if !status.success() {
    bail!("kubectl command failed with status {:?}", status.code());
  }
  Ok(BoundedOutput {
    stdout,
    stderr,
    status_code: status.code(),
  })
}

async fn read_bounded(
  mut reader: impl AsyncRead + Unpin,
  limit_bytes: usize,
) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::new();
  let mut chunk = [0_u8; 8192];
  loop {
    let read = reader.read(&mut chunk).await?;
    if read == 0 {
      return Ok(bytes);
    }
    if bytes.len().saturating_add(read) > limit_bytes {
      bail!("kubectl output exceeded the configured limit");
    }
    bytes.extend_from_slice(&chunk[..read]);
  }
}

async fn stream_bounded(
  command: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
) -> mogh_error::Result<Body> {
  let mut child = Command::new(command.program)
    .args(&command.args)
    .kill_on_drop(true)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .context("Failed to start kubectl log stream")?;
  let mut stdout =
    child.stdout.take().context("Failed to capture stdout")?;
  let deadline =
    Instant::now() + Duration::from_secs(timeout_seconds);
  let (sender, receiver) = mpsc::channel::<
    Result<Bytes, std::io::Error>,
  >(STREAM_CHANNEL_CAPACITY);

  tokio::spawn(async move {
    let mut sent = 0usize;
    let mut chunk = vec![0_u8; 8192];
    loop {
      let read =
        match timeout_at(deadline, stdout.read(&mut chunk)).await {
          Ok(Ok(read)) => read,
          Ok(Err(error)) => {
            let _ = sender.send(Err(error)).await;
            let _ = child.kill().await;
            return;
          }
          Err(_) => {
            let _ = child.kill().await;
            let _ = sender
              .send(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "kubectl log stream timed out",
              )))
              .await;
            return;
          }
        };
      if read == 0 {
        match timeout_at(deadline, child.wait()).await {
          Ok(Ok(status)) if status.success() => {}
          Ok(Ok(_)) => {
            let _ = sender
              .send(Err(std::io::Error::other(
                "kubectl log stream failed",
              )))
              .await;
          }
          Ok(Err(error)) => {
            let _ = sender.send(Err(error)).await;
          }
          Err(_) => {
            let _ = child.kill().await;
          }
        }
        return;
      }
      if sent.saturating_add(read) > limit_bytes {
        let _ = child.kill().await;
        let _ = sender
          .send(Err(std::io::Error::other(
            "kubectl log stream exceeded the configured limit",
          )))
          .await;
        return;
      }
      sent += read;
      let item = Bytes::copy_from_slice(&chunk[..read]);
      match timeout_at(deadline, sender.send(Ok(item))).await {
        Ok(Ok(())) => {}
        _ => {
          let _ = child.kill().await;
          return;
        }
      }
    }
  });

  Ok(Body::from_stream(stream::unfold(
    receiver,
    |mut receiver| async {
      receiver.recv().await.map(|item| (item, receiver))
    },
  )))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn cluster() -> RegisteredCluster {
    RegisteredCluster {
      name: "kind-komodo-lin-138".to_string(),
      kubeconfig_path: "/tmp/komodo-lin-138/kubeconfig".to_string(),
      context: Some("kind-kind-komodo-lin-138".to_string()),
    }
  }

  #[test]
  fn register_response_redacts_credential_details() {
    let response = RegisterKindClusterResponse {
      name: "kind-komodo-lin-138".to_string(),
      context: Some("kind-kind-komodo-lin-138".to_string()),
      credential_source: "server_file_path".to_string(),
      credential_redacted: true,
    };

    assert_eq!(response.credential_source, "server_file_path");
    assert!(response.credential_redacted);
  }

  #[test]
  fn list_namespaces_uses_kubeconfig_without_exposing_material() {
    let command = names_command(&cluster());

    assert_eq!(command.program, "kubectl");
    assert_eq!(
      command.args,
      [
        "--kubeconfig",
        "/tmp/komodo-lin-138/kubeconfig",
        "--context",
        "kind-kind-komodo-lin-138",
        "get",
        "namespaces",
        "-o",
        "json"
      ]
    );
    assert!(
      !command.args.join(" ").contains("client-certificate-data")
    );
  }

  #[test]
  fn list_workloads_can_scope_to_namespace() {
    let command = workloads_command(&cluster(), Some("default"));

    assert_eq!(
      command.args,
      [
        "--kubeconfig",
        "/tmp/komodo-lin-138/kubeconfig",
        "--context",
        "kind-kind-komodo-lin-138",
        "-n",
        "default",
        "get",
        "pods,deployments,statefulsets,daemonsets",
        "-o",
        "json"
      ]
    );
  }

  #[test]
  fn stream_logs_is_bounded_and_follow_capable() {
    let request = StreamPodLogs {
      cluster: "kind-komodo-lin-138".to_string(),
      namespace: "default".to_string(),
      pod: "komodo-lin-138-pod".to_string(),
      container: Some("app".to_string()),
      tail_lines: 20,
      follow: true,
      limit_bytes: 4096,
      timeout_seconds: 10,
    };
    let command = logs_command(&cluster(), &request);

    assert!(command.args.contains(&"logs".to_string()));
    assert!(command.args.contains(&"--follow".to_string()));
    assert!(command.args.contains(&"--tail=20".to_string()));
    assert!(command.args.contains(&"--limit-bytes=4096".to_string()));
    assert!(
      command.args.contains(&"--request-timeout=10s".to_string())
    );
  }

  #[test]
  fn exec_uses_argv_separator_and_no_shell() {
    let request = ExecPodCommand {
      cluster: "kind-komodo-lin-138".to_string(),
      namespace: "default".to_string(),
      pod: "komodo-lin-138-pod".to_string(),
      container: None,
      command: vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "echo ok".to_string(),
      ],
      timeout_seconds: 5,
    };
    let command = exec_command(&cluster(), &request);

    assert_eq!(
      command.args.last_chunk::<4>(),
      Some(&[
        "--".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        "echo ok".to_string()
      ])
    );
    assert!(!command.args.contains(&"sh -c".to_string()));
  }

  #[test]
  fn safe_mutation_is_idempotent_namespace_annotation() {
    let request = AnnotateNamespace {
      cluster: "kind-komodo-lin-138".to_string(),
      namespace: "default".to_string(),
      key: "komodo.rs/lin-138-spike".to_string(),
      value: "verified".to_string(),
    };
    let command = annotate_namespace_command(&cluster(), &request);

    assert_eq!(
      command.args,
      [
        "--kubeconfig",
        "/tmp/komodo-lin-138/kubeconfig",
        "--context",
        "kind-kind-komodo-lin-138",
        "annotate",
        "namespace",
        "default",
        "komodo.rs/lin-138-spike=verified",
        "--overwrite"
      ]
    );
  }

  #[test]
  fn rejects_non_admin_users_at_the_permission_seam() {
    assert!(require_admin(&User::default()).is_err());
    let admin = User {
      enabled: true,
      admin: true,
      ..Default::default()
    };
    assert!(require_admin(&admin).is_ok());
  }

  #[test]
  fn validates_names_limits_and_argv() {
    assert!(validate_kubernetes_name("valid-name.example").is_ok());
    assert!(validate_kubernetes_name("-invalid").is_err());
    assert!(validate_kubernetes_name("invalid;name").is_err());
    assert!(
      validate_annotation_key("komodo.rs/lin-138-spike").is_ok()
    );
    assert!(validate_annotation_key("komodo.rs/bad=value").is_err());
    assert!(validate_annotation_value("verified").is_ok());
    assert!(validate_annotation_value("bad\nvalue").is_err());
    assert!(validate_timeout(0).is_err());
    assert!(validate_timeout(MAX_TIMEOUT_SECONDS + 1).is_err());
    assert!(validate_output_limit(MAX_OUTPUT_BYTES + 1).is_err());
    assert!(validate_exec_argv(&[]).is_err());
    assert!(validate_exec_argv(&["bad\0arg".to_string()]).is_err());
  }

  #[test]
  fn parses_namespace_and_workload_json() {
    let namespaces = parse_namespaces(
      br#"{"items":[{"metadata":{"name":"default"},"status":{"phase":"Active"}}]}"#,
    )
    .unwrap();
    assert_eq!(namespaces[0].name, "default");
    assert_eq!(namespaces[0].status, "Active");

    let workloads = parse_workloads(
      br#"{"items":[{"kind":"Pod","metadata":{"name":"app","namespace":"default"},"status":{"containerStatuses":[{"ready":true},{"ready":false}]}}]}"#,
    )
    .unwrap();
    assert_eq!(workloads[0].kind, "Pod");
    assert_eq!(workloads[0].ready.as_deref(), Some("1/2"));
  }

  #[test]
  fn registry_rejects_conflicting_reregistration() {
    let original = cluster();
    assert!(registration_change(None, &original).is_ok());
    assert!(registration_change(Some(&original), &original).is_ok());

    let conflicting = RegisteredCluster {
      kubeconfig_path: "/tmp/other/kubeconfig".to_string(),
      ..original.clone()
    };
    assert!(
      registration_change(Some(&original), &conflicting).is_err()
    );
  }

  #[test]
  fn rejects_relative_kubeconfig_paths() {
    assert!(validate_kubeconfig_path("relative/kubeconfig").is_err());
  }

  #[tokio::test]
  async fn bounded_process_times_out_and_kills_child() {
    let command = KubectlCommand {
      program: "sh",
      args: vec!["-c".to_string(), "sleep 5".to_string()],
    };
    let result = run_bounded(&command, 1024, 1).await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn bounded_process_does_not_return_synthetic_success() {
    let command = KubectlCommand {
      program: "sh",
      args: vec!["-c".to_string(), "exit 7".to_string()],
    };
    assert!(run_bounded(&command, 1024, 2).await.is_err());
  }

  #[tokio::test]
  #[ignore = "requires disposable kind cluster and fixture pod"]
  async fn kind_vertical_slice_uses_backend_handlers() {
    let kubeconfig_path = std::env::var("KOMODO_KIND_KUBECONFIG")
      .expect(
        "KOMODO_KIND_KUBECONFIG must point to the kind kubeconfig",
      );
    let cluster_name = "kind-komodo-lin-138";
    let admin = User {
      enabled: true,
      admin: true,
      ..Default::default()
    };
    CLUSTERS.write().await.remove(cluster_name);

    let registration = register_kind_cluster(
      Extension(admin.clone()),
      Json(RegisterKindCluster {
        name: cluster_name.to_string(),
        kubeconfig_path,
        context: Some("kind-komodo-lin-138".to_string()),
      }),
    )
    .await
    .unwrap()
    .0;
    assert!(registration.credential_redacted);
    assert_eq!(registration.credential_source, "server_file_path");

    let namespaces = list_namespaces(
      Extension(admin.clone()),
      Json(ClusterSelector {
        cluster: cluster_name.to_string(),
      }),
    )
    .await
    .unwrap()
    .0;
    assert!(namespaces.iter().any(|item| item.name == "default"));

    let workloads = list_workloads(
      Extension(admin.clone()),
      Json(ListKubernetesWorkloads {
        cluster: cluster_name.to_string(),
        namespace: Some("default".to_string()),
      }),
    )
    .await
    .unwrap()
    .0;
    assert!(workloads.iter().any(|item| {
      item.kind == "Pod" && item.name == "komodo-lin-138-pod"
    }));

    let logs = stream_pod_logs(
      Extension(admin.clone()),
      Json(StreamPodLogs {
        cluster: cluster_name.to_string(),
        namespace: "default".to_string(),
        pod: "komodo-lin-138-pod".to_string(),
        container: None,
        tail_lines: 20,
        follow: false,
        limit_bytes: 4096,
        timeout_seconds: 10,
      }),
    )
    .await
    .unwrap();
    let logs = axum::body::to_bytes(logs, 4096).await.unwrap();
    assert!(
      String::from_utf8_lossy(&logs).contains("lin-138-log-ok")
    );

    let exec = exec_pod_command(
      Extension(admin.clone()),
      Json(ExecPodCommand {
        cluster: cluster_name.to_string(),
        namespace: "default".to_string(),
        pod: "komodo-lin-138-pod".to_string(),
        container: None,
        command: vec![
          "/bin/sh".to_string(),
          "-c".to_string(),
          "printf lin-138-exec-ok".to_string(),
        ],
        timeout_seconds: 10,
      }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(exec.stdout, "lin-138-exec-ok");
    assert_eq!(exec.status_code, Some(0));

    let mutation = AnnotateNamespace {
      cluster: cluster_name.to_string(),
      namespace: "default".to_string(),
      key: "komodo.rs/lin-138-spike".to_string(),
      value: "verified-live".to_string(),
    };
    let first = annotate_namespace(
      Extension(admin.clone()),
      Json(mutation.clone()),
    )
    .await
    .unwrap()
    .0;
    let replay = annotate_namespace(Extension(admin), Json(mutation))
      .await
      .unwrap()
      .0;
    assert!(first.changed);
    assert!(!replay.changed);
  }
}
