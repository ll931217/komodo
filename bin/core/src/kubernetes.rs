use std::{
  collections::HashMap,
  fs::{self, File, OpenOptions},
  io::{Read as _, Seek as _},
  os::unix::{
    fs::{
      MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    },
    process::CommandExt as _,
  },
  path::{Path, PathBuf},
  process::Stdio,
  sync::{
    Arc, LazyLock,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};

use anyhow::{Context, anyhow, bail};
use axum::{
  Extension, Router,
  body::Body,
  extract::{DefaultBodyLimit, Request},
  http::{StatusCode, header},
  middleware::{self, Next},
  response::{IntoResponse as _, Response},
  routing::post,
};
use bytes::Bytes;
use futures_util::stream;
use komodo_client::entities::user::User;
use mogh_auth_server::middleware::authenticate_request;
use mogh_error::{AddStatusCode as _, AddStatusCodeError as _, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::{
  io::{AsyncRead, AsyncReadExt},
  process::Command,
  sync::{OwnedSemaphorePermit, RwLock, Semaphore, mpsc, oneshot},
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
const MAX_KUBECTL_BYTES: u64 = 256 * 1_048_576;
const MAX_CONCURRENT_KUBECTL: usize = 8;
const STREAM_CHANNEL_CAPACITY: usize = 8;
const PROCESS_GROUP_CLEANUP_TIMEOUT: Duration =
  Duration::from_secs(1);
const PROCESS_GROUP_CLEANUP_BACKOFF: Duration =
  Duration::from_millis(10);
const KUBECONFIG_ROOT_ENV: &str = "KOMODO_KUBECONFIG_ROOT";
const KUBECTL_PATH_ENV: &str = "KOMODO_KUBECTL_PATH";
const DEFAULT_KUBECONFIG_ROOT: &str = "/etc/komodo/kubeconfigs";
const DEFAULT_KUBECTL_PATH: &str = "/usr/bin/kubectl";
const SPIKE_ANNOTATION_KEY: &str = "komodo.rs/lin-138-spike";

static CLUSTERS: LazyLock<
  RwLock<HashMap<String, RegisteredCluster>>,
> = LazyLock::new(Default::default);
static KUBECTL_PERMITS: LazyLock<Arc<Semaphore>> =
  LazyLock::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_KUBECTL)));
static NAMESPACE_MUTATION: LazyLock<tokio::sync::Mutex<()>> =
  LazyLock::new(Default::default);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredCluster {
  name: String,
  kubeconfig_path: String,
  kubeconfig_identity: FileIdentity,
  kubeconfig_digest: [u8; 32],
  kubectl: TrustedExecutable,
  context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
  device: u64,
  inode: u64,
  size: u64,
  modified_seconds: i64,
  modified_nanoseconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrustedExecutable {
  path: PathBuf,
  identity: FileIdentity,
  digest: [u8; 32],
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
  program: PathBuf,
  args: Vec<String>,
  kubeconfig: Option<RegisteredCluster>,
}

#[derive(Debug)]
struct BoundedOutput {
  stdout: Vec<u8>,
  stderr: Vec<u8>,
  status_code: Option<i32>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LogStreamEvent {
  Data { data: String },
  End,
  Error { code: String },
}

fn admin_routes() -> Router {
  Router::new()
    .route("/register-kind", post(register_kind_cluster))
    .route("/namespaces", post(list_namespaces))
    .route("/workloads", post(list_workloads))
    .route("/logs", post(stream_pod_logs))
    .route("/exec", post(exec_pod_command))
    .route("/annotate-namespace", post(annotate_namespace))
    .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
    .layer(middleware::from_fn(require_admin_request))
}

pub fn router() -> Router {
  admin_routes().layer(middleware::from_fn(
    authenticate_request::<KomodoAuthImpl, true>,
  ))
}

async fn require_admin_request(
  request: Request,
  next: Next,
) -> mogh_error::Result<Response> {
  let user = request.extensions().get::<User>().ok_or_else(|| {
    anyhow!("Authentication is required")
      .status_code(StatusCode::UNAUTHORIZED)
  })?;
  require_admin(user)?;
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
  let approved_root = approved_kubeconfig_root()
    .status_code(StatusCode::INTERNAL_SERVER_ERROR)?;
  let (kubeconfig_path, kubeconfig_identity, kubeconfig_digest) =
    validate_kubeconfig_path(
      Path::new(&request.kubeconfig_path),
      &approved_root,
    )
    .status_code(StatusCode::BAD_REQUEST)?;
  let kubectl = validate_kubectl_executable(&kubectl_path())
    .status_code(StatusCode::INTERNAL_SERVER_ERROR)?;
  let cluster = RegisteredCluster {
    name: request.name.clone(),
    kubeconfig_path,
    kubeconfig_identity,
    kubeconfig_digest,
    kubectl,
    context: request.context.clone(),
  };

  {
    let clusters = CLUSTERS.read().await;
    registration_change(clusters.get(&request.name), &cluster)
      .status_code(StatusCode::CONFLICT)?;
  }

  let output = run_authorized(
    &[auth_can_i_command(&cluster, "list", "namespaces", None)],
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
  let output = run_authorized(
    &[auth_can_i_command(&cluster, "list", "namespaces", None)],
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
  let preflights =
    ["pods", "deployments", "statefulsets", "daemonsets"].map(
      |resource| {
        auth_can_i_command(
          &cluster,
          "list",
          resource,
          request.namespace.as_deref(),
        )
      },
    );
  let output = run_authorized(
    &preflights,
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
) -> mogh_error::Result<Response> {
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
  let deadline =
    Instant::now() + Duration::from_secs(request.timeout_seconds);
  let cluster = get_cluster(&request.cluster).await?;
  preflight_allowed(
    &auth_can_i_command(
      &cluster,
      "get",
      "pods/log",
      Some(&request.namespace),
    ),
    deadline,
  )
  .await
  .map_err(kubectl_http_error)?;
  let body = stream_bounded_until(
    &logs_command(&cluster, &request),
    request.limit_bytes,
    deadline,
    KUBECTL_PERMITS.clone(),
  )
  .await?;
  Ok(
    ([(header::CONTENT_TYPE, "application/x-ndjson")], body)
      .into_response(),
  )
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
  let output = run_authorized(
    &[auth_can_i_command(
      &cluster,
      "create",
      "pods/exec",
      Some(&request.namespace),
    )],
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
  validate_spike_annotation_key(&request.key)
    .status_code(StatusCode::BAD_REQUEST)?;
  validate_annotation_value(&request.value)
    .status_code(StatusCode::BAD_REQUEST)?;
  let deadline =
    Instant::now() + Duration::from_secs(default_timeout_seconds());
  let cluster = get_cluster(&request.cluster).await?;
  let annotation = format!("{}={}", request.key, request.value);
  let _mutation = timeout_at(deadline, NAMESPACE_MUTATION.lock())
    .await
    .context("Kubernetes annotation request timed out")
    .map_err(kubectl_http_error)?;

  let current = run_authorized_until(
    &[auth_can_i_command(&cluster, "get", "namespaces", None)],
    &read_annotation_command(&cluster, &request),
    4096,
    deadline,
  )
  .await
  .map_err(kubectl_http_error)?;
  let current = parse_annotation_state(&current.stdout, &request.key)
    .map_err(kubectl_http_error)?;
  let changed =
    annotation_changed(current.as_deref(), &request.value);
  if changed {
    run_authorized_until(
      &[auth_can_i_command(&cluster, "patch", "namespaces", None)],
      &annotate_namespace_command(&cluster, &request),
      4096,
      deadline,
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
  let contains = |needle: &str| {
    error
      .chain()
      .any(|cause| cause.to_string().contains(needle))
  };
  let status = if contains("timed out") {
    StatusCode::GATEWAY_TIMEOUT
  } else if contains("exceeded the configured limit")
    || contains("too many items")
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

fn validate_spike_annotation_key(value: &str) -> anyhow::Result<()> {
  validate_annotation_key(value)?;
  if value != SPIKE_ANNOTATION_KEY {
    bail!("Only the fixed spike annotation key is allowed");
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

fn approved_kubeconfig_root() -> anyhow::Result<PathBuf> {
  let configured = std::env::var_os(KUBECONFIG_ROOT_ENV)
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(DEFAULT_KUBECONFIG_ROOT));
  configured
    .canonicalize()
    .context("Failed to resolve the approved kubeconfig root")
}

fn validate_kubeconfig_path(
  value: &Path,
  approved_root: &Path,
) -> anyhow::Result<(String, FileIdentity, [u8; 32])> {
  if !value.is_absolute() {
    bail!(
      "kubeconfig_path must be an absolute server-local file path"
    );
  }
  let path = value
    .canonicalize()
    .context("Failed to resolve kubeconfig_path")?;
  if !path.starts_with(approved_root) {
    bail!("kubeconfig_path is outside the operator-approved root");
  }
  validate_trusted_directories(
    approved_root,
    path.parent().unwrap_or(approved_root),
  )?;
  let mut file = OpenOptions::new()
    .read(true)
    .custom_flags(0x20000 | 0x80000)
    .open(&path)
    .context("Failed to open kubeconfig_path safely")?;
  let metadata = file
    .metadata()
    .context("Failed to inspect kubeconfig_path")?;
  validate_kubeconfig_metadata(&metadata)?;
  let mut bytes = Vec::with_capacity(metadata.len() as usize);
  file
    .read_to_end(&mut bytes)
    .context("Failed to read kubeconfig_path")?;
  validate_kubeconfig_document(&bytes)?;
  let path = path
    .to_str()
    .map(str::to_owned)
    .ok_or_else(|| anyhow!("kubeconfig_path must be valid UTF-8"))?;
  Ok((path, file_identity(&metadata), sha256(&bytes)))
}

fn validate_trusted_directories(
  root: &Path,
  parent: &Path,
) -> anyhow::Result<()> {
  let euid = unsafe { libc_geteuid() };
  let mut current = Some(parent);
  while let Some(path) = current {
    let metadata = fs::symlink_metadata(path)
      .context("Failed to inspect a kubeconfig directory")?;
    if !metadata.file_type().is_dir()
      || metadata.uid() != 0 && metadata.uid() != euid
      || metadata.permissions().mode() & 0o022 != 0
    {
      bail!("Kubeconfig directory trust policy failed");
    }
    if path == root {
      return Ok(());
    }
    current = path.parent();
  }
  bail!("Kubeconfig parent is outside the approved root")
}

fn validate_kubeconfig_metadata(
  metadata: &fs::Metadata,
) -> anyhow::Result<()> {
  if !metadata.file_type().is_file() {
    bail!("kubeconfig_path must identify a regular file");
  }
  if metadata.len() == 0 || metadata.len() > MAX_KUBECONFIG_BYTES {
    bail!(
      "kubeconfig_path must identify a non-empty file no larger than {MAX_KUBECONFIG_BYTES} bytes"
    );
  }
  if metadata.uid() != unsafe { libc_geteuid() } {
    bail!("kubeconfig_path must be owned by the Core process user");
  }
  if metadata.permissions().mode() & 0o077 != 0 {
    bail!(
      "kubeconfig_path must not grant group or other permissions"
    );
  }
  Ok(())
}

fn validate_kubeconfig_document(bytes: &[u8]) -> anyhow::Result<()> {
  let document: serde_yaml_ng::Value =
    serde_yaml_ng::from_slice(bytes)
      .context("Failed to parse kubeconfig_path")?;
  if document
    .get("clusters")
    .and_then(|clusters| clusters.as_sequence())
    .into_iter()
    .flatten()
    .filter_map(|cluster| cluster.get("cluster"))
    .any(|cluster| cluster.get("certificate-authority").is_some())
  {
    bail!("External kubeconfig file references are not allowed");
  }
  let Some(users) =
    document.get("users").and_then(|users| users.as_sequence())
  else {
    return Ok(());
  };
  for user in users {
    let Some(credentials) = user.get("user") else {
      continue;
    };
    if credentials.get("exec").is_some()
      || credentials.get("auth-provider").is_some()
    {
      bail!(
        "kubeconfig exec and auth-provider plugins are not allowed"
      );
    }
    if ["client-certificate", "client-key", "tokenFile"]
      .iter()
      .any(|key| credentials.get(*key).is_some())
    {
      bail!("External kubeconfig file references are not allowed");
    }
  }
  Ok(())
}

unsafe extern "C" {
  fn geteuid() -> u32;
}

unsafe fn libc_geteuid() -> u32 {
  unsafe { geteuid() }
}

fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
  FileIdentity {
    device: metadata.dev(),
    inode: metadata.ino(),
    size: metadata.len(),
    modified_seconds: metadata.mtime(),
    modified_nanoseconds: metadata.mtime_nsec(),
  }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
  Sha256::digest(bytes).into()
}

fn read_bounded_file(
  file: &mut File,
  limit: u64,
) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::new();
  file
    .take(limit + 1)
    .read_to_end(&mut bytes)
    .context("Failed to read trusted file")?;
  if bytes.len() as u64 > limit {
    bail!("Trusted file exceeded the configured limit");
  }
  Ok(bytes)
}

fn hash_bounded_file(
  file: &mut File,
  limit: u64,
) -> anyhow::Result<[u8; 32]> {
  let mut digest = Sha256::new();
  let mut buffer = [0_u8; 8192];
  let mut remaining = limit;
  loop {
    let read_limit =
      usize::try_from(remaining.min(buffer.len() as u64))
        .unwrap_or(buffer.len());
    let read = file
      .read(&mut buffer[..read_limit])
      .context("Failed to read trusted file")?;
    if read == 0 {
      return Ok(digest.finalize().into());
    }
    digest.update(&buffer[..read]);
    remaining -= read as u64;
    if remaining == 0 {
      let mut overflow = [0_u8; 1];
      if file
        .read(&mut overflow)
        .context("Failed to read trusted file")?
        != 0
      {
        bail!("Trusted file exceeded the configured limit");
      }
      return Ok(digest.finalize().into());
    }
  }
}

fn open_registered_kubeconfig(
  cluster: &RegisteredCluster,
) -> anyhow::Result<File> {
  let mut file = OpenOptions::new()
    .read(true)
    .custom_flags(0x20000 | 0x80000)
    .open(&cluster.kubeconfig_path)
    .context("Failed to reopen registered kubeconfig")?;
  let metadata = file.metadata()?;
  validate_kubeconfig_metadata(&metadata)?;
  let bytes = read_bounded_file(&mut file, MAX_KUBECONFIG_BYTES)?;
  validate_kubeconfig_document(&bytes)?;
  if file_identity(&metadata) != cluster.kubeconfig_identity
    || sha256(&bytes) != cluster.kubeconfig_digest
  {
    bail!("Registered kubeconfig was replaced or modified");
  }
  file
    .rewind()
    .context("Failed to rewind registered kubeconfig")?;
  Ok(file)
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

fn kubectl_path() -> PathBuf {
  std::env::var_os(KUBECTL_PATH_ENV)
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(DEFAULT_KUBECTL_PATH))
}

fn validate_kubectl_metadata(
  metadata: &fs::Metadata,
) -> anyhow::Result<()> {
  let euid = unsafe { libc_geteuid() };
  if !metadata.file_type().is_file()
    || metadata.uid() != 0 && metadata.uid() != euid
    || metadata.permissions().mode() & 0o022 != 0
    || metadata.permissions().mode() & 0o111 == 0
  {
    bail!("Configured kubectl executable failed trust policy");
  }
  Ok(())
}

fn validate_kubectl_executable(
  path: &Path,
) -> anyhow::Result<TrustedExecutable> {
  if !path.is_absolute() {
    bail!("Configured kubectl path must be absolute");
  }
  let path = path
    .canonicalize()
    .context("Failed to resolve configured kubectl executable")?;
  let mut file = OpenOptions::new()
    .read(true)
    .custom_flags(0x20000 | 0x80000)
    .open(&path)
    .context("Failed to open configured kubectl executable")?;
  let metadata = file
    .metadata()
    .context("Failed to inspect configured kubectl executable")?;
  validate_kubectl_metadata(&metadata)?;
  if metadata.len() > MAX_KUBECTL_BYTES {
    bail!("Configured kubectl executable exceeded the size limit");
  }
  let digest = hash_bounded_file(&mut file, MAX_KUBECTL_BYTES)?;
  Ok(TrustedExecutable {
    path,
    identity: file_identity(&metadata),
    digest,
  })
}

fn open_registered_kubectl(
  executable: &TrustedExecutable,
) -> anyhow::Result<File> {
  let mut file = OpenOptions::new()
    .read(true)
    .custom_flags(0x20000 | 0x80000)
    .open(&executable.path)
    .context("Failed to reopen configured kubectl executable")?;
  let metadata = file.metadata()?;
  validate_kubectl_metadata(&metadata)?;
  if metadata.len() > MAX_KUBECTL_BYTES {
    bail!("Configured kubectl executable exceeded the size limit");
  }
  let digest = hash_bounded_file(&mut file, MAX_KUBECTL_BYTES)?;
  if file_identity(&metadata) != executable.identity
    || digest != executable.digest
  {
    bail!("Configured kubectl executable was replaced or modified");
  }
  file
    .rewind()
    .context("Failed to rewind configured kubectl executable")?;
  Ok(file)
}

fn cluster_args(cluster: &RegisteredCluster) -> Vec<String> {
  let mut args =
    vec!["--kubeconfig".to_string(), "/proc/self/fd/3".to_string()];
  if let Some(context) = &cluster.context {
    args.extend(["--context".to_string(), context.clone()]);
  }
  args
}

fn auth_can_i_command(
  cluster: &RegisteredCluster,
  verb: &str,
  resource: &str,
  namespace: Option<&str>,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "auth".to_string(),
    "can-i".to_string(),
    verb.to_string(),
    resource.to_string(),
  ]);
  if let Some(namespace) = namespace {
    args.extend(["--namespace".to_string(), namespace.to_string()]);
  } else if resource != "namespaces" {
    args.push("--all-namespaces".to_string());
  }
  KubectlCommand {
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
  }
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
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
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
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
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
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
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
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
  }
}

fn read_annotation_command(
  cluster: &RegisteredCluster,
  request: &AnnotateNamespace,
) -> KubectlCommand {
  let mut args = cluster_args(cluster);
  args.extend([
    "get".to_string(),
    "namespace".to_string(),
    request.namespace.clone(),
    "-o".to_string(),
    "json".to_string(),
  ]);
  KubectlCommand {
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
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
    program: cluster.kubectl.path.clone(),
    args,
    kubeconfig: Some(cluster.clone()),
  }
}

fn parse_annotation_state(
  bytes: &[u8],
  key: &str,
) -> anyhow::Result<Option<String>> {
  let document: Value = serde_json::from_slice(bytes)
    .context("Failed to parse kubectl annotation output")?;
  let annotations = document["metadata"]["annotations"].as_object();
  match annotations.and_then(|annotations| annotations.get(key)) {
    None | Some(Value::Null) => Ok(None),
    Some(Value::String(value)) => Ok(Some(value.clone())),
    Some(_) => {
      bail!("kubectl annotation output has an invalid shape")
    }
  }
}

fn annotation_changed(
  current: Option<&str>,
  requested: &str,
) -> bool {
  current != Some(requested)
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

async fn acquire_kubectl_permit(
  deadline: Instant,
  permits: Arc<Semaphore>,
) -> anyhow::Result<OwnedSemaphorePermit> {
  timeout_at(deadline, permits.acquire_owned())
    .await
    .context("kubectl command timed out waiting for capacity")?
    .context("kubectl process limiter is closed")
}

fn spawn_command(
  command: &KubectlCommand,
  capture_stderr: bool,
) -> anyhow::Result<tokio::process::Child> {
  let trusted = command
    .kubeconfig
    .as_ref()
    .map(|cluster| cluster.kubectl.clone())
    .map_or_else(
      || validate_kubectl_executable(&command.program),
      Ok,
    )?;
  let executable = open_registered_kubectl(&trusted)?;
  let kubeconfig = command
    .kubeconfig
    .as_ref()
    .map(open_registered_kubeconfig)
    .transpose()?;
  let mut process = Command::new("/proc/self/fd/4");
  process
    .args(&command.args)
    .kill_on_drop(true)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(if capture_stderr {
      Stdio::piped()
    } else {
      Stdio::null()
    });
  unsafe {
    process.as_std_mut().pre_exec(move || {
      if setpgid(0, 0) == -1 {
        return Err(std::io::Error::last_os_error());
      }
      duplicate_command_descriptors(&executable, kubeconfig.as_ref())
    });
  }
  process.spawn().context("Failed to spawn kubectl")
}

fn duplicate_command_descriptors(
  executable: &File,
  kubeconfig: Option<&File>,
) -> std::io::Result<()> {
  let executable = std::os::fd::AsRawFd::as_raw_fd(executable);
  let kubeconfig = kubeconfig.map(std::os::fd::AsRawFd::as_raw_fd);
  let executable_temporary = duplicate_temporary_if(
    executable,
    executable == 3 && kubeconfig.is_some(),
  )?;
  let kubeconfig_temporary =
    kubeconfig.map(|fd| duplicate_temporary_if(fd, fd == 4));
  let kubeconfig_temporary = kubeconfig_temporary.transpose()?;
  duplicate_inheritable(executable_temporary, 4)?;
  if let Some(kubeconfig) = kubeconfig_temporary {
    duplicate_inheritable(kubeconfig, 3)?;
  }
  for temporary in
    [executable_temporary, kubeconfig_temporary.unwrap_or(-1)]
  {
    if temporary > 4 && unsafe { close(temporary) } == -1 {
      return Err(std::io::Error::last_os_error());
    }
  }
  Ok(())
}

fn duplicate_temporary_if(
  source: i32,
  collision: bool,
) -> std::io::Result<i32> {
  if !collision {
    return Ok(source);
  }
  let temporary = unsafe { fcntl(source, F_DUPFD_CLOEXEC, 5) };
  if temporary == -1 {
    return Err(std::io::Error::last_os_error());
  }
  Ok(temporary)
}

fn duplicate_inheritable(
  source: i32,
  target: i32,
) -> std::io::Result<()> {
  if source != target && unsafe { dup2(source, target) } == -1 {
    return Err(std::io::Error::last_os_error());
  }
  if unsafe { fcntl(target, F_SETFD, 0) } == -1 {
    return Err(std::io::Error::last_os_error());
  }
  Ok(())
}

const F_SETFD: i32 = 2;
const F_DUPFD_CLOEXEC: i32 = 1030;

unsafe extern "C" {
  fn close(fd: i32) -> i32;
  fn dup2(old_fd: i32, new_fd: i32) -> i32;
  fn fcntl(fd: i32, command: i32, argument: i32) -> i32;
  fn kill(pid: i32, signal: i32) -> i32;
  fn setpgid(pid: i32, pgid: i32) -> i32;
}

fn process_group_id(
  child: &tokio::process::Child,
) -> anyhow::Result<i32> {
  let pid =
    child.id().context("Spawned kubectl process has no ID")?;
  i32::try_from(pid).context("kubectl process ID exceeded i32")
}

async fn terminate_and_reap(
  child: &mut tokio::process::Child,
  process_group_id: i32,
) {
  let process_group = -process_group_id;
  let _ = unsafe { kill(process_group, 15) };
  tokio::task::yield_now().await;
  let _ = unsafe { kill(process_group, 9) };
  let _ = child.wait().await;
  let cleanup_deadline =
    Instant::now() + PROCESS_GROUP_CLEANUP_TIMEOUT;
  while unsafe { kill(process_group, 0) } == 0
    && Instant::now() < cleanup_deadline
  {
    tokio::time::sleep(PROCESS_GROUP_CLEANUP_BACKOFF).await;
  }
}

async fn preflight_allowed(
  command: &KubectlCommand,
  deadline: Instant,
) -> anyhow::Result<()> {
  // `kubectl auth can-i` may emit a harmless scope warning on stderr for
  // cluster-scoped resources. Keep that output bounded while parsing only the
  // exact stdout decision.
  let output = run_bounded_until(command, 4096, deadline).await?;
  if output.stdout == b"yes\n" || output.stdout == b"yes" {
    Ok(())
  } else {
    bail!(
      "Kubernetes RBAC preflight denied or returned malformed output"
    )
  }
}

async fn run_authorized(
  preflights: &[KubectlCommand],
  operation: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
) -> anyhow::Result<BoundedOutput> {
  validate_timeout(timeout_seconds)?;
  let deadline =
    Instant::now() + Duration::from_secs(timeout_seconds);
  run_authorized_until(preflights, operation, limit_bytes, deadline)
    .await
}

async fn run_authorized_until(
  preflights: &[KubectlCommand],
  operation: &KubectlCommand,
  limit_bytes: usize,
  deadline: Instant,
) -> anyhow::Result<BoundedOutput> {
  for preflight in preflights {
    preflight_allowed(preflight, deadline).await?;
  }
  run_bounded_until(operation, limit_bytes, deadline).await
}

#[cfg(test)]
async fn run_bounded(
  command: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
) -> anyhow::Result<BoundedOutput> {
  validate_timeout(timeout_seconds)?;
  let deadline =
    Instant::now() + Duration::from_secs(timeout_seconds);
  run_bounded_until(command, limit_bytes, deadline).await
}

async fn run_bounded_until(
  command: &KubectlCommand,
  limit_bytes: usize,
  deadline: Instant,
) -> anyhow::Result<BoundedOutput> {
  run_bounded_until_with_permits(
    command,
    limit_bytes,
    deadline,
    KUBECTL_PERMITS.clone(),
  )
  .await
}

async fn run_bounded_until_with_permits(
  command: &KubectlCommand,
  limit_bytes: usize,
  deadline: Instant,
  permits: Arc<Semaphore>,
) -> anyhow::Result<BoundedOutput> {
  validate_output_limit(limit_bytes)?;
  let permit = acquire_kubectl_permit(deadline, permits).await?;
  let mut child = spawn_command(command, true)
    .context("Failed to start kubectl process")?;
  let process_group_id = process_group_id(&child)?;
  let stdout =
    child.stdout.take().context("Failed to capture stdout")?;
  let stderr =
    child.stderr.take().context("Failed to capture stderr")?;
  let (mut sender, receiver) = oneshot::channel();

  tokio::spawn(async move {
    let budget = Arc::new(AtomicUsize::new(limit_bytes));
    let operation = async {
      let (status, stdout, stderr) = tokio::try_join!(
        async {
          child.wait().await.context("Failed to wait for kubectl")
        },
        read_bounded(stdout, budget.clone()),
        read_bounded(stderr, budget),
      )?;
      anyhow::Ok((status, stdout, stderr))
    };
    let result = tokio::select! {
      biased;
      _ = sender.closed() => {
        terminate_and_reap(&mut child, process_group_id).await;
        return;
      }
      result = timeout_at(deadline, operation) => match result {
        Ok(Ok((status, stdout, stderr))) if status.success() => {
          terminate_and_reap(&mut child, process_group_id).await;
          Ok(BoundedOutput {
            stdout,
            stderr,
            status_code: status.code(),
          })
        }
        Ok(Ok((status, _, _))) => {
          terminate_and_reap(&mut child, process_group_id).await;
          Err(anyhow!(
            "kubectl command failed with status {:?}",
            status.code()
          ))
        }
        Ok(Err(error)) => {
          terminate_and_reap(&mut child, process_group_id).await;
          Err(error).context("kubectl command failed")
        }
        Err(_) => {
          terminate_and_reap(&mut child, process_group_id).await;
          Err(anyhow!("kubectl command timed out"))
        }
      }
    };
    let _ = sender.send(result);
    drop(permit);
  });

  receiver
    .await
    .context("kubectl command supervisor stopped unexpectedly")?
}

async fn read_bounded(
  mut reader: impl AsyncRead + Unpin,
  remaining: Arc<AtomicUsize>,
) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::new();
  let mut chunk = [0_u8; 8192];
  loop {
    let read = reader.read(&mut chunk).await?;
    if read == 0 {
      return Ok(bytes);
    }
    if remaining
      .fetch_update(
        Ordering::AcqRel,
        Ordering::Acquire,
        |remaining| remaining.checked_sub(read),
      )
      .is_err()
    {
      bail!("kubectl output exceeded the configured limit");
    }
    bytes.extend_from_slice(&chunk[..read]);
  }
}

#[cfg(test)]
async fn stream_bounded(
  command: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
) -> mogh_error::Result<Body> {
  stream_bounded_with_permits(
    command,
    limit_bytes,
    timeout_seconds,
    KUBECTL_PERMITS.clone(),
  )
  .await
}

#[cfg(test)]
async fn stream_bounded_with_permits(
  command: &KubectlCommand,
  limit_bytes: usize,
  timeout_seconds: u64,
  permits: Arc<Semaphore>,
) -> mogh_error::Result<Body> {
  let deadline =
    Instant::now() + Duration::from_secs(timeout_seconds);
  stream_bounded_until(command, limit_bytes, deadline, permits).await
}

async fn stream_bounded_until(
  command: &KubectlCommand,
  limit_bytes: usize,
  deadline: Instant,
  permits: Arc<Semaphore>,
) -> mogh_error::Result<Body> {
  let permit = acquire_kubectl_permit(deadline, permits).await?;
  let mut child = spawn_command(command, false)
    .context("Failed to start kubectl log stream")?;
  let process_group_id = process_group_id(&child)?;
  let mut stdout =
    child.stdout.take().context("Failed to capture stdout")?;
  let (sender, receiver) = mpsc::channel::<
    Result<Bytes, std::io::Error>,
  >(STREAM_CHANNEL_CAPACITY);
  let (terminal_sender, terminal_receiver) = oneshot::channel();

  tokio::spawn(async move {
    let _permit = permit;
    let mut terminal_sender = Some(terminal_sender);
    let mut sent = 0usize;
    let mut chunk = vec![0_u8; 8192];
    let mut pending_utf8 = Vec::new();
    loop {
      let read = tokio::select! {
        _ = sender.closed() => {
          terminate_and_reap(&mut child, process_group_id).await;
          return;
        }
        _ = tokio::time::sleep_until(deadline) => {
          terminate_and_reap(&mut child, process_group_id).await;
          send_stream_event(
            terminal_sender.take(),
            LogStreamEvent::Error { code: "timeout".to_string() },
          );
          return;
        }
        read = stdout.read(&mut chunk) => match read {
          Ok(read) => read,
          Err(_) => {
            terminate_and_reap(&mut child, process_group_id).await;
            send_stream_event(
              terminal_sender.take(),
              LogStreamEvent::Error { code: "read_failed".to_string() },
            );
            return;
          }
        }
      };
      if read == 0 {
        if !pending_utf8.is_empty() {
          let item = stream_event_bytes(LogStreamEvent::Data {
            data: String::from_utf8_lossy(&pending_utf8).into_owned(),
          });
          if !send_stream_item(
            &sender,
            item,
            deadline,
            &mut child,
            process_group_id,
            &mut terminal_sender,
          )
          .await
          {
            return;
          }
        }
        let event = match timeout_at(deadline, child.wait()).await {
          Ok(Ok(status)) if status.success() => LogStreamEvent::End,
          Ok(Ok(_)) => LogStreamEvent::Error {
            code: "process_failed".to_string(),
          },
          Ok(Err(_)) => LogStreamEvent::Error {
            code: "wait_failed".to_string(),
          },
          Err(_) => LogStreamEvent::Error {
            code: "timeout".to_string(),
          },
        };
        terminate_and_reap(&mut child, process_group_id).await;
        send_stream_event(terminal_sender.take(), event);
        return;
      }
      if sent.saturating_add(read) > limit_bytes {
        terminate_and_reap(&mut child, process_group_id).await;
        send_stream_event(
          terminal_sender.take(),
          LogStreamEvent::Error {
            code: "output_limit".to_string(),
          },
        );
        return;
      }
      sent += read;
      pending_utf8.extend_from_slice(&chunk[..read]);
      let valid_bytes = match std::str::from_utf8(&pending_utf8) {
        Ok(_) => pending_utf8.len(),
        Err(error) if error.error_len().is_none() => {
          error.valid_up_to()
        }
        Err(_) => pending_utf8.len(),
      };
      if valid_bytes == 0 {
        continue;
      }
      let item = stream_event_bytes(LogStreamEvent::Data {
        data: String::from_utf8_lossy(&pending_utf8[..valid_bytes])
          .into_owned(),
      });
      pending_utf8.drain(..valid_bytes);
      if !send_stream_item(
        &sender,
        item,
        deadline,
        &mut child,
        process_group_id,
        &mut terminal_sender,
      )
      .await
      {
        return;
      }
    }
  });

  Ok(Body::from_stream(stream::unfold(
    (receiver, Some(terminal_receiver)),
    |(mut receiver, mut terminal_receiver)| async move {
      if let Some(item) = receiver.recv().await {
        return Some((item, (receiver, terminal_receiver)));
      }
      let terminal_receiver = terminal_receiver.take()?;
      terminal_receiver
        .await
        .ok()
        .map(|item| (item, (receiver, None)))
    },
  )))
}

async fn send_stream_item(
  sender: &mpsc::Sender<Result<Bytes, std::io::Error>>,
  item: Bytes,
  deadline: Instant,
  child: &mut tokio::process::Child,
  process_group_id: i32,
  terminal_sender: &mut Option<
    oneshot::Sender<Result<Bytes, std::io::Error>>,
  >,
) -> bool {
  let timed_out = tokio::select! {
    _ = sender.closed() => false,
    _ = tokio::time::sleep_until(deadline) => true,
    result = sender.send(Ok(item)) => {
      if result.is_ok() {
        return true;
      }
      false
    }
  };
  terminate_and_reap(child, process_group_id).await;
  if timed_out {
    send_stream_event(
      terminal_sender.take(),
      LogStreamEvent::Error {
        code: "timeout".to_string(),
      },
    );
  }
  false
}

fn stream_event_bytes(event: LogStreamEvent) -> Bytes {
  let mut bytes = serde_json::to_vec(&event).unwrap_or_else(|_| {
    br#"{"type":"error","code":"framing_failed"}"#.to_vec()
  });
  bytes.push(b'\n');
  Bytes::from(bytes)
}

fn send_stream_event(
  sender: Option<oneshot::Sender<Result<Bytes, std::io::Error>>>,
  event: LogStreamEvent,
) {
  if let Some(sender) = sender {
    let _ = sender.send(Ok(stream_event_bytes(event)));
  }
}

#[cfg(test)]
mod tests {
  use std::{
    fs,
    os::{
      fd::{AsRawFd as _, FromRawFd as _},
      unix::{fs::PermissionsExt, process::CommandExt as _},
    },
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
  };

  use axum::{
    extract::Request,
    http::{Method, StatusCode},
  };
  use tower::ServiceExt;

  use super::*;

  static TEST_ID: AtomicU64 = AtomicU64::new(0);

  fn test_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
      "komodo-kubernetes-test-{}-{}",
      std::process::id(),
      TEST_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    // create_dir takes the mode from the caller's umask, and the
    // policy under test rejects any group- or other-writable
    // directory. A developer with umask 002 gets 0775 here and every
    // "this should validate" assertion fails - not because the policy
    // is wrong, but because the fixture is. Pin it.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
      .unwrap();
    path
  }

  fn write_file(path: &Path, contents: &str, mode: u32) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
      .unwrap();
  }

  #[test]
  fn descriptor_remapping_handles_source_collisions() {
    for (executable_source, kubeconfig_source) in
      [(3, 4), (3, 5), (5, 4), (5, 6), (4, 3)]
    {
      let directory = test_dir();
      let executable_path = directory.join("executable");
      let kubeconfig_path = directory.join("kubeconfig");
      write_file(&executable_path, "executable", 0o600);
      write_file(&kubeconfig_path, "kubeconfig", 0o600);
      let executable = File::open(executable_path).unwrap();
      let kubeconfig = File::open(kubeconfig_path).unwrap();
      let executable_fd = executable.as_raw_fd();
      let kubeconfig_fd = kubeconfig.as_raw_fd();

      let mut child = std::process::Command::new("/bin/sh");
      child
        .arg("-c")
        .arg("printf '%s|' \"$(cat <&4)\"; cat <&3")
        .stdout(Stdio::piped());
      unsafe {
        child.pre_exec(move || {
          let saved_executable =
            fcntl(executable_fd, F_DUPFD_CLOEXEC, 10);
          let saved_kubeconfig =
            fcntl(kubeconfig_fd, F_DUPFD_CLOEXEC, 10);
          if saved_executable == -1 || saved_kubeconfig == -1 {
            return Err(std::io::Error::last_os_error());
          }
          if dup2(saved_executable, executable_source) == -1
            || dup2(saved_kubeconfig, kubeconfig_source) == -1
            || close(saved_executable) == -1
            || close(saved_kubeconfig) == -1
          {
            return Err(std::io::Error::last_os_error());
          }
          for source in [executable_source, kubeconfig_source] {
            if source > 4 && fcntl(source, F_SETFD, 1) == -1 {
              return Err(std::io::Error::last_os_error());
            }
          }
          let executable = File::from_raw_fd(executable_source);
          let kubeconfig = File::from_raw_fd(kubeconfig_source);
          duplicate_command_descriptors(
            &executable,
            Some(&kubeconfig),
          )?;
          std::mem::forget(executable);
          std::mem::forget(kubeconfig);
          Ok(())
        });
      }

      let output = child.output().unwrap();
      assert!(
        output.status.success(),
        "sources: executable={executable_source}, kubeconfig={kubeconfig_source}"
      );
      assert_eq!(
        output.stdout, b"executable|kubeconfig",
        "sources: executable={executable_source}, kubeconfig={kubeconfig_source}"
      );
    }
  }

  fn cluster() -> RegisteredCluster {
    RegisteredCluster {
      name: "kind-komodo-lin-138".to_string(),
      kubeconfig_path: "/tmp/komodo-lin-138/kubeconfig".to_string(),
      kubeconfig_identity: FileIdentity {
        device: 1,
        inode: 2,
        size: 3,
        modified_seconds: 4,
        modified_nanoseconds: 5,
      },
      kubeconfig_digest: [0; 32],
      kubectl: TrustedExecutable {
        path: PathBuf::from(DEFAULT_KUBECTL_PATH),
        identity: FileIdentity {
          device: 1,
          inode: 2,
          size: 3,
          modified_seconds: 4,
          modified_nanoseconds: 5,
        },
        digest: [0; 32],
      },
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

    assert!(command.program.is_absolute());
    assert_eq!(
      command.args,
      [
        "--kubeconfig",
        "/proc/self/fd/3",
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
        "/proc/self/fd/3",
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
  fn rbac_preflights_are_operation_specific() {
    assert_eq!(
      auth_can_i_command(&cluster(), "list", "namespaces", None).args,
      [
        "--kubeconfig",
        "/proc/self/fd/3",
        "--context",
        "kind-kind-komodo-lin-138",
        "auth",
        "can-i",
        "list",
        "namespaces"
      ]
    );
    assert_eq!(
      auth_can_i_command(
        &cluster(),
        "get",
        "pods/log",
        Some("default")
      )
      .args,
      [
        "--kubeconfig",
        "/proc/self/fd/3",
        "--context",
        "kind-kind-komodo-lin-138",
        "auth",
        "can-i",
        "get",
        "pods/log",
        "--namespace",
        "default"
      ]
    );
    assert_eq!(
      auth_can_i_command(&cluster(), "list", "pods", None).args,
      [
        "--kubeconfig",
        "/proc/self/fd/3",
        "--context",
        "kind-kind-komodo-lin-138",
        "auth",
        "can-i",
        "list",
        "pods",
        "--all-namespaces"
      ]
    );
  }

  #[test]
  fn mutation_rejects_non_spike_annotation_keys() {
    assert!(
      validate_spike_annotation_key(SPIKE_ANNOTATION_KEY).is_ok()
    );
    assert!(
      validate_spike_annotation_key("example.com/arbitrary").is_err()
    );
  }

  #[tokio::test]
  async fn denied_preflight_never_runs_operation() {
    let directory = test_dir();
    let executable = directory.join("kubectl");
    let marker = directory.join("operation-ran");
    write_file(
      &executable,
      &format!(
        "#!/bin/sh\nif [ \"$1\" = auth ]; then printf no; exit 1; fi\nprintf ran > '{}'\n",
        marker.display()
      ),
      0o700,
    );
    let preflight = KubectlCommand {
      program: executable.clone(),
      args: vec!["auth".to_string()],
      kubeconfig: None,
    };
    let operation = KubectlCommand {
      program: executable,
      args: vec!["get".to_string()],
      kubeconfig: None,
    };

    assert!(
      run_authorized(&[preflight], &operation, 1024, 2)
        .await
        .is_err()
    );
    assert!(!marker.exists());
  }

  #[tokio::test]
  async fn authorized_request_uses_one_end_to_end_deadline() {
    let directory = test_dir();
    let executable = directory.join("kubectl");
    write_file(
      &executable,
      "#!/bin/sh\nsleep 0.6\nif [ \"$1\" = auth ]; then printf yes; fi\n",
      0o700,
    );
    let preflight = KubectlCommand {
      program: executable.clone(),
      args: vec!["auth".to_string()],
      kubeconfig: None,
    };
    let operation = KubectlCommand {
      program: executable,
      args: vec!["get".to_string()],
      kubeconfig: None,
    };

    let error = run_authorized(&[preflight], &operation, 1024, 1)
      .await
      .unwrap_err();

    assert!(format!("{error:#}").contains("timed out"));
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
        "/proc/self/fd/3",
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

  #[tokio::test]
  async fn wrapped_output_limit_maps_to_sanitized_payload_too_large()
  {
    let error =
      anyhow!("kubectl output exceeded the configured limit")
        .context("kubectl command failed");
    let response = kubectl_http_error(error).into_response();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = axum::body::to_bytes(response.into_body(), 1024)
      .await
      .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("Kubernetes operation failed"));
    assert!(!body.contains("configured limit"));
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
  fn rejects_unapproved_or_unsafe_kubeconfigs() {
    let root = test_dir();
    let safe = root.join("safe.yaml");
    write_file(
      &safe,
      "apiVersion: v1\nkind: Config\nusers: [{name: safe, user: {token: test}}]\n",
      0o600,
    );
    // Not a bare is_ok(): when this fails, the reason is the whole
    // point, and an assert that prints nothing sent one debugging
    // session after the kubeconfig contents instead of the path.
    if let Err(e) = validate_kubeconfig_path(&safe, &root) {
      panic!("a safe kubeconfig must validate, got: {e:#}");
    }

    let outside = test_dir().join("outside.yaml");
    write_file(&outside, "apiVersion: v1\nkind: Config\n", 0o600);
    assert!(validate_kubeconfig_path(&outside, &root).is_err());

    let executable = root.join("exec.yaml");
    write_file(
      &executable,
      "apiVersion: v1\nkind: Config\nusers: [{name: unsafe, user: {exec: {command: sh}}}]\n",
      0o600,
    );
    assert!(validate_kubeconfig_path(&executable, &root).is_err());

    let auth_provider = root.join("auth-provider.yaml");
    write_file(
      &auth_provider,
      "apiVersion: v1\nkind: Config\nusers: [{name: unsafe, user: {auth-provider: {name: oidc}}}]\n",
      0o600,
    );
    assert!(validate_kubeconfig_path(&auth_provider, &root).is_err());

    let permissive = root.join("permissive.yaml");
    write_file(&permissive, "apiVersion: v1\nkind: Config\n", 0o644);
    assert!(validate_kubeconfig_path(&permissive, &root).is_err());
  }

  #[test]
  fn trust_boundary_rejects_external_refs_unsafe_ancestors_and_binary()
   {
    let parent = test_dir();
    let root = parent.join("credentials");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
      .unwrap();
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o777))
      .unwrap();
    let config = nested.join("config");
    write_file(&config, "apiVersion: v1\nkind: Config\n", 0o600);
    assert!(validate_kubeconfig_path(&config, &root).is_err());

    for reference in [
      "certificate-authority",
      "client-certificate",
      "client-key",
      "tokenFile",
    ] {
      let document = if reference == "certificate-authority" {
        format!(
          "apiVersion: v1\nkind: Config\nclusters: [{{name: x, cluster: {{{reference}: /tmp/x}}}}]\n"
        )
      } else {
        format!(
          "apiVersion: v1\nkind: Config\nusers: [{{name: x, user: {{{reference}: /tmp/x}}}}]\n"
        )
      };
      assert!(
        validate_kubeconfig_document(document.as_bytes()).is_err()
      );
    }

    let executable = parent.join("kubectl");
    write_file(&executable, "#!/bin/sh\nexit 0\n", 0o700);
    assert_eq!(
      validate_kubectl_executable(&executable).unwrap().path,
      executable
    );
    fs::set_permissions(
      &executable,
      fs::Permissions::from_mode(0o722),
    )
    .unwrap();
    assert!(validate_kubectl_executable(&executable).is_err());
  }

  #[test]
  fn oversized_kubectl_is_rejected_at_registration_and_spawn() {
    let directory = test_dir();
    let executable = directory.join("kubectl");
    write_file(&executable, "#!/bin/sh\nexit 0\n", 0o700);
    let trusted = validate_kubectl_executable(&executable).unwrap();
    let oversized =
      File::options().write(true).open(&executable).unwrap();
    oversized.set_len(MAX_KUBECTL_BYTES + 1).unwrap();

    assert!(validate_kubectl_executable(&executable).is_err());
    assert!(open_registered_kubectl(&trusted).is_err());
  }

  #[test]
  fn registered_kubectl_modification_is_rejected() {
    let directory = test_dir();
    let executable = directory.join("kubectl");
    write_file(&executable, "#!/bin/sh\nexit 0\n", 0o700);
    let trusted = validate_kubectl_executable(&executable).unwrap();
    write_file(&executable, "#!/bin/sh\nexit 1\n", 0o700);

    assert!(open_registered_kubectl(&trusted).is_err());
  }

  #[test]
  fn registered_kubeconfig_in_place_modification_is_rejected() {
    let root = test_dir();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
      .unwrap();
    let config = root.join("config");
    let original = "apiVersion: v1\nkind: Config\nusers: []\n";
    let modified = "apiVersion: v1\nkind: Config\nusers: {}\n";
    assert_eq!(original.len(), modified.len());
    write_file(&config, original, 0o600);
    let (path, identity, digest) =
      validate_kubeconfig_path(&config, &root).unwrap();
    let registered = RegisteredCluster {
      name: "fixture".to_string(),
      kubeconfig_path: path,
      kubeconfig_identity: identity,
      kubeconfig_digest: digest,
      kubectl: validate_kubectl_executable(Path::new("/bin/sh"))
        .unwrap(),
      context: None,
    };
    write_file(&config, modified, 0o600);

    assert!(open_registered_kubeconfig(&registered).is_err());
  }

  #[test]
  fn registered_kubeconfig_replacement_is_rejected() {
    let root = test_dir();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
      .unwrap();
    let config = root.join("config");
    write_file(&config, "apiVersion: v1\nkind: Config\n", 0o600);
    let (path, identity, digest) =
      validate_kubeconfig_path(&config, &root).unwrap();
    let registered = RegisteredCluster {
      name: "fixture".to_string(),
      kubeconfig_path: path,
      kubeconfig_identity: identity,
      kubeconfig_digest: digest,
      kubectl: validate_kubectl_executable(Path::new("/bin/sh"))
        .unwrap(),
      context: None,
    };
    fs::remove_file(&config).unwrap();
    write_file(
      &config,
      "apiVersion: v1\nkind: Config\nusers: []\n",
      0o600,
    );
    assert!(open_registered_kubeconfig(&registered).is_err());
  }

  #[test]
  fn annotation_state_distinguishes_absent_and_empty() {
    assert!(annotation_changed(None, ""));
    assert!(!annotation_changed(Some(""), ""));
    assert!(!annotation_changed(Some("verified"), "verified"));
    assert!(annotation_changed(Some("old"), "verified"));
    assert!(!annotation_changed(Some("verified"), "verified"));
  }

  #[tokio::test]
  async fn route_gate_rejects_disabled_and_non_admin_but_allows_admin()
   {
    fn gated(user: Option<User>) -> Router {
      user.map_or_else(admin_routes, |user| {
        admin_routes().layer(Extension(user))
      })
    }

    let request = || {
      Request::builder()
        .method(Method::POST)
        .uri("/namespaces")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"cluster":"missing"}"#))
        .unwrap()
    };
    assert_eq!(
      gated(None).oneshot(request()).await.unwrap().status(),
      StatusCode::UNAUTHORIZED
    );
    let disabled_admin = User {
      admin: true,
      ..Default::default()
    };
    assert_eq!(
      gated(Some(disabled_admin))
        .oneshot(request())
        .await
        .unwrap()
        .status(),
      StatusCode::FORBIDDEN
    );
    let non_admin = User {
      enabled: true,
      ..Default::default()
    };
    assert_eq!(
      gated(Some(non_admin))
        .oneshot(request())
        .await
        .unwrap()
        .status(),
      StatusCode::FORBIDDEN
    );
    let admin = User {
      enabled: true,
      admin: true,
      ..Default::default()
    };
    assert_eq!(
      gated(Some(admin))
        .oneshot(request())
        .await
        .unwrap()
        .status(),
      StatusCode::NOT_FOUND
    );
    let super_admin = User {
      enabled: true,
      super_admin: true,
      ..Default::default()
    };
    assert_eq!(
      gated(Some(super_admin))
        .oneshot(request())
        .await
        .unwrap()
        .status(),
      StatusCode::NOT_FOUND
    );
  }

  #[tokio::test]
  async fn public_router_rejects_unauthenticated_requests() {
    let response = router()
      .oneshot(
        Request::builder()
          .method(Method::POST)
          .uri("/namespaces")
          .header(header::CONTENT_TYPE, "application/json")
          .body(Body::from(r#"{"cluster":"missing"}"#))
          .unwrap(),
      )
      .await
      .unwrap();
    assert!(matches!(
      response.status(),
      StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ));
  }

  #[tokio::test]
  async fn concurrent_annotation_replay_is_truthful() {
    let directory = test_dir();
    fs::set_permissions(
      &directory,
      fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let kubeconfig = directory.join("kubeconfig");
    write_file(&kubeconfig, "apiVersion: v1\nkind: Config\n", 0o600);
    let executable = directory.join("kubectl");
    let state = directory.join("state");
    write_file(
      &executable,
      &format!(
        "#!/bin/sh\ncase \"$*\" in\n  *'auth can-i'*) printf yes;;\n  *'get namespace'*) if [ -f '{0}' ]; then printf '{{\"metadata\":{{\"annotations\":{{\"{1}\":\"verified\"}}}}}}'; else printf '{{\"metadata\":{{\"annotations\":{{}}}}}}'; fi;;\n  *'annotate namespace'*) sleep 0.1; : > '{0}';;\nesac\n",
        state.display(),
        SPIKE_ANNOTATION_KEY,
      ),
      0o700,
    );
    let (path, identity, digest) =
      validate_kubeconfig_path(&kubeconfig, &directory).unwrap();
    let name = format!(
      "concurrent-{}",
      TEST_ID.fetch_add(1, Ordering::Relaxed)
    );
    CLUSTERS.write().await.insert(
      name.clone(),
      RegisteredCluster {
        name: name.clone(),
        kubeconfig_path: path,
        kubeconfig_identity: identity,
        kubeconfig_digest: digest,
        kubectl: validate_kubectl_executable(&executable).unwrap(),
        context: None,
      },
    );
    let admin = User {
      enabled: true,
      admin: true,
      ..Default::default()
    };
    let mutation = AnnotateNamespace {
      cluster: name.clone(),
      namespace: "default".to_string(),
      key: SPIKE_ANNOTATION_KEY.to_string(),
      value: "verified".to_string(),
    };
    let (first, second) = tokio::join!(
      annotate_namespace(
        Extension(admin.clone()),
        Json(mutation.clone())
      ),
      annotate_namespace(Extension(admin), Json(mutation)),
    );
    let mut changed =
      [first.unwrap().0.changed, second.unwrap().0.changed];
    changed.sort_unstable();
    assert_eq!(changed, [false, true]);
    CLUSTERS.write().await.remove(&name);
  }

  #[tokio::test]
  async fn log_stream_uses_ndjson_data_end_and_sanitized_errors() {
    async fn collect(
      script: &str,
      limit: usize,
      timeout: u64,
    ) -> Vec<LogStreamEvent> {
      let command = KubectlCommand {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_string(), script.to_string()],
        kubeconfig: None,
      };
      let body =
        stream_bounded(&command, limit, timeout).await.unwrap();
      let bytes = axum::body::to_bytes(body, 16_384).await.unwrap();
      let text = String::from_utf8(bytes.to_vec()).unwrap();
      text
        .lines()
        .map(|line| {
          serde_json::from_str::<LogStreamEvent>(line).unwrap()
        })
        .collect()
    }

    let normal = collect("printf hello", 128, 2).await;
    assert!(
      matches!(&normal[0], LogStreamEvent::Data { data } if data == "hello")
    );
    assert!(matches!(normal[1], LogStreamEvent::End));

    let limited = collect("printf 123456789", 4, 2).await;
    assert!(matches!(
      limited.last(),
      Some(LogStreamEvent::Error { code }) if code == "output_limit"
    ));
    let failed = collect("exit 9", 128, 2).await;
    assert!(matches!(
      failed.last(),
      Some(LogStreamEvent::Error { code }) if code == "process_failed"
    ));
    let timed_out = collect("sleep 5", 128, 1).await;
    assert!(matches!(
      timed_out.last(),
      Some(LogStreamEvent::Error { code }) if code == "timeout"
    ));
  }

  #[tokio::test]
  async fn log_stream_preserves_utf8_across_read_boundaries() {
    let expected = format!("{}{}", "0".repeat(8191), '\u{20ac}');
    let command = KubectlCommand {
      program: PathBuf::from("/usr/bin/printf"),
      args: vec![format!("%08191d{}", '\u{20ac}'), "0".to_string()],
      kubeconfig: None,
    };
    let body = stream_bounded(&command, 16_384, 2).await.unwrap();
    let bytes = axum::body::to_bytes(body, 32_768).await.unwrap();
    let streamed = String::from_utf8(bytes.to_vec())
      .unwrap()
      .lines()
      .filter_map(
        |line| match serde_json::from_str::<LogStreamEvent>(line)
          .unwrap()
        {
          LogStreamEvent::Data { data } => Some(data),
          LogStreamEvent::End => None,
          LogStreamEvent::Error { code } => {
            panic!("unexpected stream error: {code}")
          }
        },
      )
      .collect::<String>();

    assert_eq!(streamed, expected);
  }

  #[tokio::test]
  async fn terminal_stream_backpressure_releases_permit() {
    let permits = Arc::new(Semaphore::new(1));
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        format!(
          "i=0; while [ $i -lt {} ]; do printf x; sleep 0.01; i=$((i + 1)); done; exit 9",
          STREAM_CHANNEL_CAPACITY + 1
        ),
      ],
      kubeconfig: None,
    };
    let body = stream_bounded_with_permits(
      &command,
      STREAM_CHANNEL_CAPACITY,
      5,
      permits.clone(),
    )
    .await
    .unwrap();

    let permit = timeout_at(
      Instant::now() + Duration::from_millis(500),
      permits.clone().acquire_owned(),
    )
    .await
    .expect("terminal event backpressure retained the kubectl permit")
    .unwrap();
    drop(permit);

    let bytes = axum::body::to_bytes(body, 16_384).await.unwrap();
    let events = String::from_utf8(bytes.to_vec())
      .unwrap()
      .lines()
      .map(|line| {
        serde_json::from_str::<LogStreamEvent>(line).unwrap()
      })
      .collect::<Vec<_>>();
    assert!(matches!(
      events.last(),
      Some(LogStreamEvent::Error { code }) if code == "output_limit"
    ));
  }

  #[tokio::test]
  async fn saturated_log_stream_delivers_terminal_timeout() {
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        format!(
          "i=0; while [ $i -lt {} ]; do printf x; sleep 0.05; i=$((i + 1)); done; sleep 10",
          STREAM_CHANNEL_CAPACITY + 1
        ),
      ],
      kubeconfig: None,
    };
    let body =
      stream_bounded(&command, STREAM_CHANNEL_CAPACITY + 1, 1)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let bytes = axum::body::to_bytes(body, 16_384).await.unwrap();
    let events = String::from_utf8(bytes.to_vec())
      .unwrap()
      .lines()
      .map(|line| {
        serde_json::from_str::<LogStreamEvent>(line).unwrap()
      })
      .collect::<Vec<_>>();

    assert_eq!(
      events
        .iter()
        .filter(|event| matches!(event, LogStreamEvent::Data { .. }))
        .count(),
      STREAM_CHANNEL_CAPACITY
    );
    assert!(matches!(
      events.last(),
      Some(LogStreamEvent::Error { code }) if code == "timeout"
    ));
  }

  #[tokio::test]
  async fn quiet_log_disconnect_reaps_child_and_releases_permit() {
    let directory = test_dir();
    let pid_file = directory.join("pid");
    let permits = Arc::new(Semaphore::new(1));
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        "printf $$ > \"$1\"; sleep 10".to_string(),
        "test".to_string(),
        pid_file.to_string_lossy().into_owned(),
      ],
      kubeconfig: None,
    };
    let body =
      stream_bounded_with_permits(&command, 128, 5, permits.clone())
        .await
        .unwrap();
    timeout_at(Instant::now() + Duration::from_secs(1), async {
      while !pid_file.exists() {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    drop(body);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let pid = fs::read_to_string(pid_file).unwrap();
    assert!(!Path::new("/proc").join(pid.trim()).exists());
    assert_eq!(permits.available_permits(), 1);
  }

  #[tokio::test]
  async fn log_disconnect_reaps_descendant_before_releasing_permit() {
    let directory = test_dir();
    let parent_pid_file = directory.join("parent-pid");
    let descendant_pid_file = directory.join("descendant-pid");
    let permits = Arc::new(Semaphore::new(1));
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        "printf $$ > \"$1\"; sleep 10 & printf $! > \"$2\"; wait"
          .to_string(),
        "test".to_string(),
        parent_pid_file.to_string_lossy().into_owned(),
        descendant_pid_file.to_string_lossy().into_owned(),
      ],
      kubeconfig: None,
    };
    let body =
      stream_bounded_with_permits(&command, 128, 5, permits.clone())
        .await
        .unwrap();
    timeout_at(Instant::now() + Duration::from_secs(1), async {
      while !parent_pid_file.exists() || !descendant_pid_file.exists()
      {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();

    drop(body);
    let permit = timeout_at(
      Instant::now() + Duration::from_secs(1),
      permits.clone().acquire_owned(),
    )
    .await
    .expect("kubectl permit was not released")
    .unwrap();
    let parent_pid = fs::read_to_string(parent_pid_file).unwrap();
    let descendant_pid =
      fs::read_to_string(descendant_pid_file).unwrap();

    assert!(!Path::new("/proc").join(parent_pid.trim()).exists());
    assert!(!Path::new("/proc").join(descendant_pid.trim()).exists());
    drop(permit);
  }

  #[tokio::test]
  async fn failed_bounded_process_reaps_group_before_releasing_permit()
   {
    let directory = test_dir();
    let parent_pid_file = directory.join("parent-pid");
    let descendant_pid_file = directory.join("descendant-pid");
    let permits = Arc::new(Semaphore::new(1));
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        "printf $$ > \"$1\"; sleep 10 </dev/null >/dev/null 2>&1 & printf $! > \"$2\"; exit 9"
          .to_string(),
        "test".to_string(),
        parent_pid_file.to_string_lossy().into_owned(),
        descendant_pid_file.to_string_lossy().into_owned(),
      ],
      kubeconfig: None,
    };

    let result = run_bounded_until_with_permits(
      &command,
      128,
      Instant::now() + Duration::from_secs(5),
      permits.clone(),
    )
    .await;
    assert!(result.is_err());
    let permit = timeout_at(
      Instant::now() + Duration::from_secs(1),
      permits.acquire_owned(),
    )
    .await
    .expect("kubectl permit was not safely released")
    .unwrap();
    let parent_pid = fs::read_to_string(parent_pid_file).unwrap();
    let descendant_pid =
      fs::read_to_string(descendant_pid_file).unwrap();

    assert!(!Path::new("/proc").join(parent_pid.trim()).exists());
    assert!(!Path::new("/proc").join(descendant_pid.trim()).exists());
    drop(permit);
  }

  #[tokio::test]
  async fn cancelled_bounded_process_reaps_group_before_releasing_permit()
   {
    let directory = test_dir();
    let parent_pid_file = directory.join("parent-pid");
    let descendant_pid_file = directory.join("descendant-pid");
    let permits = Arc::new(Semaphore::new(1));
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        "printf $$ > \"$1\"; sleep 10 & printf $! > \"$2\"; wait"
          .to_string(),
        "test".to_string(),
        parent_pid_file.to_string_lossy().into_owned(),
        descendant_pid_file.to_string_lossy().into_owned(),
      ],
      kubeconfig: None,
    };
    let task = tokio::spawn({
      let permits = permits.clone();
      async move {
        run_bounded_until_with_permits(
          &command,
          128,
          Instant::now() + Duration::from_secs(5),
          permits,
        )
        .await
      }
    });
    timeout_at(Instant::now() + Duration::from_secs(1), async {
      while !parent_pid_file.exists() || !descendant_pid_file.exists()
      {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();

    task.abort();
    task.await.unwrap_err();
    let permit = timeout_at(
      Instant::now() + Duration::from_secs(1),
      permits.clone().acquire_owned(),
    )
    .await
    .expect("kubectl permit was not safely released")
    .unwrap();
    let parent_pid = fs::read_to_string(parent_pid_file).unwrap();
    let descendant_pid =
      fs::read_to_string(descendant_pid_file).unwrap();

    assert!(!Path::new("/proc").join(parent_pid.trim()).exists());
    assert!(!Path::new("/proc").join(descendant_pid.trim()).exists());
    drop(permit);
  }

  #[tokio::test]
  async fn bounded_process_enforces_aggregate_output_limit() {
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec![
        "-c".to_string(),
        "printf 123456; printf abcdef >&2".to_string(),
      ],
      kubeconfig: None,
    };
    assert!(run_bounded(&command, 10, 2).await.is_err());

    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec!["-c".to_string(), "printf 12345678901".to_string()],
      kubeconfig: None,
    };
    assert!(run_bounded(&command, 10, 2).await.is_err());
  }

  #[tokio::test]
  async fn bounded_process_timeout_and_limit_reap_descendants() {
    for script in [
      "printf $$ > \"$1\"; sleep 10 & printf $! > \"$2\"; wait",
      "printf $$ > \"$1\"; sleep 10 & printf $! > \"$2\"; yes x",
    ] {
      let directory = test_dir();
      let parent_pid_file = directory.join("parent-pid");
      let descendant_pid_file = directory.join("descendant-pid");
      let permits = Arc::new(Semaphore::new(1));
      let command = KubectlCommand {
        program: PathBuf::from("/bin/sh"),
        args: vec![
          "-c".to_string(),
          script.to_string(),
          "test".to_string(),
          parent_pid_file.to_string_lossy().into_owned(),
          descendant_pid_file.to_string_lossy().into_owned(),
        ],
        kubeconfig: None,
      };
      let limit = if script.contains("yes") { 128 } else { 1024 };
      let result = run_bounded_until_with_permits(
        &command,
        limit,
        Instant::now() + Duration::from_secs(1),
        permits.clone(),
      )
      .await;
      assert!(result.is_err());
      let permit = timeout_at(
        Instant::now() + Duration::from_secs(1),
        permits.acquire_owned(),
      )
      .await
      .expect("kubectl permit was not safely released")
      .unwrap();
      let parent_pid = fs::read_to_string(parent_pid_file).unwrap();
      let descendant_pid =
        fs::read_to_string(descendant_pid_file).unwrap();

      assert!(!Path::new("/proc").join(parent_pid.trim()).exists());
      assert!(
        !Path::new("/proc").join(descendant_pid.trim()).exists()
      );
      drop(permit);
    }
  }

  #[tokio::test]
  async fn bounded_process_times_out_and_kills_child() {
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec!["-c".to_string(), "sleep 5".to_string()],
      kubeconfig: None,
    };
    let result = run_bounded(&command, 1024, 1).await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn bounded_process_does_not_return_synthetic_success() {
    let command = KubectlCommand {
      program: PathBuf::from("/bin/sh"),
      args: vec!["-c".to_string(), "exit 7".to_string()],
      kubeconfig: None,
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
        timeout_seconds: 30,
      }),
    )
    .await
    .unwrap();
    assert_eq!(
      logs.headers()[header::CONTENT_TYPE],
      "application/x-ndjson"
    );
    let logs =
      axum::body::to_bytes(logs.into_body(), 4096).await.unwrap();
    let events = String::from_utf8_lossy(&logs);
    assert!(events.contains("lin-138-log-ok"), "events: {events}");
    assert!(events.contains("\"type\":\"end\""), "events: {events}");

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
        timeout_seconds: 30,
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
      value: format!("verified-live-{}", std::process::id()),
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
