use serde::{Deserialize, Serialize};
use typeshare::typeshare;

use super::ResourceTarget;

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum ProviderKind {
  Docker,
  Compose,
  Swarm,
  Kubernetes,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum ResourceKind {
  Container,
  DockerDeployment,
  ComposeProject,
  SwarmService,
  SwarmStack,
  KubernetesWorkload,
  KubernetesPod,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum KubernetesWorkloadKind {
  Deployment,
  StatefulSet,
  DaemonSet,
  Job,
  CronJob,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum KubernetesResourceKind {
  Deployment,
  StatefulSet,
  DaemonSet,
  Job,
  CronJob,
  Pod,
}

impl From<KubernetesWorkloadKind> for KubernetesResourceKind {
  fn from(kind: KubernetesWorkloadKind) -> Self {
    match kind {
      KubernetesWorkloadKind::Deployment => Self::Deployment,
      KubernetesWorkloadKind::StatefulSet => Self::StatefulSet,
      KubernetesWorkloadKind::DaemonSet => Self::DaemonSet,
      KubernetesWorkloadKind::Job => Self::Job,
      KubernetesWorkloadKind::CronJob => Self::CronJob,
    }
  }
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct DockerResourceLocator {
  pub server: String,
  pub name: String,
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct KubernetesWorkloadLocator {
  pub cluster: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub namespace: Option<String>,
  pub kind: KubernetesWorkloadKind,
  pub name: String,
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct KubernetesPodLocator {
  pub cluster: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub namespace: Option<String>,
  pub kind: KubernetesPodKind,
  pub name: String,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum KubernetesPodKind {
  Pod,
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(tag = "type", content = "id")]
pub enum DeploymentResourceLocator {
  Deployment(String),
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(tag = "type", content = "id")]
pub enum StackResourceLocator {
  Stack(String),
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(tag = "type", content = "params")]
pub enum ResourceLocator {
  Komodo(ResourceTarget),
  Docker(DockerResourceLocator),
  Kubernetes {
    cluster: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    kind: KubernetesResourceKind,
    name: String,
  },
}

#[typeshare]
#[derive(
  Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(tag = "kind", content = "resource")]
pub enum ResourceIdentity {
  Container {
    provider: DockerProvider,
    locator: DockerResourceLocator,
  },
  DockerDeployment {
    provider: DockerProvider,
    locator: DeploymentResourceLocator,
  },
  ComposeProject {
    provider: ComposeProvider,
    locator: StackResourceLocator,
  },
  SwarmService {
    provider: SwarmProvider,
    locator: DeploymentResourceLocator,
  },
  SwarmStack {
    provider: SwarmProvider,
    locator: StackResourceLocator,
  },
  KubernetesWorkload {
    provider: KubernetesProvider,
    locator: KubernetesWorkloadLocator,
  },
  KubernetesPod {
    provider: KubernetesProvider,
    locator: KubernetesPodLocator,
  },
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum DockerProvider {
  Docker,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum ComposeProvider {
  Compose,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum SwarmProvider {
  Swarm,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum KubernetesProvider {
  Kubernetes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyAdapterError {
  provider: ProviderKind,
  target: ResourceTarget,
}

impl LegacyAdapterError {
  pub fn provider(&self) -> ProviderKind {
    self.provider
  }
  pub fn target(&self) -> &ResourceTarget {
    &self.target
  }
  pub fn into_target(self) -> ResourceTarget {
    self.target
  }
}

impl ResourceIdentity {
  pub fn provider(&self) -> ProviderKind {
    match self {
      Self::Container { .. } | Self::DockerDeployment { .. } => {
        ProviderKind::Docker
      }
      Self::ComposeProject { .. } => ProviderKind::Compose,
      Self::SwarmService { .. } | Self::SwarmStack { .. } => {
        ProviderKind::Swarm
      }
      Self::KubernetesWorkload { .. }
      | Self::KubernetesPod { .. } => ProviderKind::Kubernetes,
    }
  }

  pub fn kind(&self) -> ResourceKind {
    match self {
      Self::Container { .. } => ResourceKind::Container,
      Self::DockerDeployment { .. } => ResourceKind::DockerDeployment,
      Self::ComposeProject { .. } => ResourceKind::ComposeProject,
      Self::SwarmService { .. } => ResourceKind::SwarmService,
      Self::SwarmStack { .. } => ResourceKind::SwarmStack,
      Self::KubernetesWorkload { .. } => {
        ResourceKind::KubernetesWorkload
      }
      Self::KubernetesPod { .. } => ResourceKind::KubernetesPod,
    }
  }

  pub fn locator(&self) -> ResourceLocator {
    match self {
      Self::Container { locator, .. } => {
        ResourceLocator::Docker(locator.clone())
      }
      Self::DockerDeployment {
        locator: DeploymentResourceLocator::Deployment(id),
        ..
      }
      | Self::SwarmService {
        locator: DeploymentResourceLocator::Deployment(id),
        ..
      } => ResourceLocator::Komodo(ResourceTarget::Deployment(
        id.clone(),
      )),
      Self::ComposeProject {
        locator: StackResourceLocator::Stack(id),
        ..
      }
      | Self::SwarmStack {
        locator: StackResourceLocator::Stack(id),
        ..
      } => ResourceLocator::Komodo(ResourceTarget::Stack(id.clone())),
      Self::KubernetesWorkload { locator, .. } => {
        ResourceLocator::Kubernetes {
          cluster: locator.cluster.clone(),
          namespace: locator.namespace.clone(),
          kind: locator.kind.into(),
          name: locator.name.clone(),
        }
      }
      Self::KubernetesPod { locator, .. } => {
        ResourceLocator::Kubernetes {
          cluster: locator.cluster.clone(),
          namespace: locator.namespace.clone(),
          kind: KubernetesResourceKind::Pod,
          name: locator.name.clone(),
        }
      }
    }
  }

  pub fn docker_container(
    server: impl Into<String>,
    name: impl Into<String>,
  ) -> Self {
    Self::Container {
      provider: DockerProvider::Docker,
      locator: DockerResourceLocator {
        server: server.into(),
        name: name.into(),
      },
    }
  }

  pub fn kubernetes_workload(
    cluster: impl Into<String>,
    namespace: Option<&str>,
    kind: KubernetesWorkloadKind,
    name: impl Into<String>,
  ) -> Self {
    Self::KubernetesWorkload {
      provider: KubernetesProvider::Kubernetes,
      locator: KubernetesWorkloadLocator {
        cluster: cluster.into(),
        namespace: namespace.map(str::to_string),
        kind,
        name: name.into(),
      },
    }
  }

  pub fn kubernetes_pod(
    cluster: impl Into<String>,
    namespace: Option<&str>,
    name: impl Into<String>,
  ) -> Self {
    Self::KubernetesPod {
      provider: KubernetesProvider::Kubernetes,
      locator: KubernetesPodLocator {
        cluster: cluster.into(),
        namespace: namespace.map(str::to_string),
        kind: KubernetesPodKind::Pod,
        name: name.into(),
      },
    }
  }

  pub fn kubernetes(
    cluster: impl Into<String>,
    namespace: Option<&str>,
    kind: KubernetesResourceKind,
    name: impl Into<String>,
  ) -> Self {
    let cluster = cluster.into();
    let name = name.into();
    match kind {
      KubernetesResourceKind::Pod => {
        Self::kubernetes_pod(cluster, namespace, name)
      }
      KubernetesResourceKind::Deployment => {
        Self::kubernetes_workload(
          cluster,
          namespace,
          KubernetesWorkloadKind::Deployment,
          name,
        )
      }
      KubernetesResourceKind::StatefulSet => {
        Self::kubernetes_workload(
          cluster,
          namespace,
          KubernetesWorkloadKind::StatefulSet,
          name,
        )
      }
      KubernetesResourceKind::DaemonSet => Self::kubernetes_workload(
        cluster,
        namespace,
        KubernetesWorkloadKind::DaemonSet,
        name,
      ),
      KubernetesResourceKind::Job => Self::kubernetes_workload(
        cluster,
        namespace,
        KubernetesWorkloadKind::Job,
        name,
      ),
      KubernetesResourceKind::CronJob => Self::kubernetes_workload(
        cluster,
        namespace,
        KubernetesWorkloadKind::CronJob,
        name,
      ),
    }
  }

  pub fn from_deployment(
    provider: ProviderKind,
    target: ResourceTarget,
  ) -> Result<Self, LegacyAdapterError> {
    let ResourceTarget::Deployment(id) = target else {
      return Err(LegacyAdapterError { provider, target });
    };
    match provider {
      ProviderKind::Docker => Ok(Self::DockerDeployment {
        provider: DockerProvider::Docker,
        locator: DeploymentResourceLocator::Deployment(id),
      }),
      ProviderKind::Swarm => Ok(Self::SwarmService {
        provider: SwarmProvider::Swarm,
        locator: DeploymentResourceLocator::Deployment(id),
      }),
      _ => Err(LegacyAdapterError {
        provider,
        target: ResourceTarget::Deployment(id),
      }),
    }
  }

  pub fn from_stack(
    provider: ProviderKind,
    target: ResourceTarget,
  ) -> Result<Self, LegacyAdapterError> {
    let ResourceTarget::Stack(id) = target else {
      return Err(LegacyAdapterError { provider, target });
    };
    match provider {
      ProviderKind::Compose => Ok(Self::ComposeProject {
        provider: ComposeProvider::Compose,
        locator: StackResourceLocator::Stack(id),
      }),
      ProviderKind::Swarm => Ok(Self::SwarmStack {
        provider: SwarmProvider::Swarm,
        locator: StackResourceLocator::Stack(id),
      }),
      _ => Err(LegacyAdapterError {
        provider,
        target: ResourceTarget::Stack(id),
      }),
    }
  }
}

impl TryFrom<ResourceIdentity> for ResourceTarget {
  type Error = ResourceIdentity;
  fn try_from(
    identity: ResourceIdentity,
  ) -> Result<Self, Self::Error> {
    match identity {
      ResourceIdentity::DockerDeployment {
        locator: DeploymentResourceLocator::Deployment(id),
        ..
      }
      | ResourceIdentity::SwarmService {
        locator: DeploymentResourceLocator::Deployment(id),
        ..
      } => Ok(Self::Deployment(id)),
      ResourceIdentity::ComposeProject {
        locator: StackResourceLocator::Stack(id),
        ..
      }
      | ResourceIdentity::SwarmStack {
        locator: StackResourceLocator::Stack(id),
        ..
      } => Ok(Self::Stack(id)),
      identity => Err(identity),
    }
  }
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum ResourceStatus {
  Unknown,
  Pending,
  Running,
  Paused,
  Stopped,
  Failed,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum ResourceRelationshipKind {
  Parent,
  Child,
  RunsOn,
  MemberOf,
  DependsOn,
}

#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRelationship {
  pub kind: ResourceRelationshipKind,
  pub resource: ResourceIdentity,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum ResourceAction {
  Deploy,
  Pull,
  Start,
  Restart,
  Pause,
  Unpause,
  Stop,
  Delete,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum CapabilityState {
  Enabled,
  UnsupportedHidden,
  UnsupportedDisabled,
}

impl CapabilityState {
  pub fn enabled(self) -> bool {
    self == Self::Enabled
  }
  pub fn visible(self) -> bool {
    self != Self::UnsupportedHidden
  }
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct ActionCapabilities {
  pub deploy: CapabilityState,
  pub pull: CapabilityState,
  pub start: CapabilityState,
  pub restart: CapabilityState,
  pub pause: CapabilityState,
  pub unpause: CapabilityState,
  pub stop: CapabilityState,
  pub delete: CapabilityState,
}

impl ActionCapabilities {
  pub fn action(&self, action: ResourceAction) -> CapabilityState {
    match action {
      ResourceAction::Deploy => self.deploy,
      ResourceAction::Pull => self.pull,
      ResourceAction::Start => self.start,
      ResourceAction::Restart => self.restart,
      ResourceAction::Pause => self.pause,
      ResourceAction::Unpause => self.unpause,
      ResourceAction::Stop => self.stop,
      ResourceAction::Delete => self.delete,
    }
  }
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct StreamingCapabilities {
  pub logs: CapabilityState,
  pub terminal: CapabilityState,
}

#[typeshare]
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct ProviderCapabilities {
  pub actions: ActionCapabilities,
  pub streaming: StreamingCapabilities,
}

impl ProviderCapabilities {
  pub fn action(&self, action: ResourceAction) -> CapabilityState {
    self.actions.action(action)
  }
}

#[typeshare]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderResource {
  pub identity: ResourceIdentity,
  pub status: ResourceStatus,
  pub relationships: Vec<ResourceRelationship>,
  pub capabilities: ProviderCapabilities,
}

pub fn provider_capabilities(
  provider: ProviderKind,
  kind: ResourceKind,
) -> Option<ProviderCapabilities> {
  use CapabilityState::{
    Enabled, UnsupportedDisabled, UnsupportedHidden,
  };
  use ResourceAction::*;
  let (enabled, streaming) = match (provider, kind) {
    (ProviderKind::Docker, ResourceKind::Container) => (
      &[Start, Restart, Pause, Unpause, Stop, Delete][..],
      StreamingCapabilities {
        logs: Enabled,
        terminal: Enabled,
      },
    ),
    (ProviderKind::Docker, ResourceKind::DockerDeployment)
    | (ProviderKind::Compose, ResourceKind::ComposeProject) => (
      &[Deploy, Pull, Start, Restart, Pause, Unpause, Stop, Delete][..],
      StreamingCapabilities {
        logs: Enabled,
        terminal: Enabled,
      },
    ),
    (ProviderKind::Swarm, ResourceKind::SwarmService)
    | (ProviderKind::Swarm, ResourceKind::SwarmStack) => (
      &[Deploy, Delete][..],
      StreamingCapabilities {
        logs: Enabled,
        terminal: UnsupportedDisabled,
      },
    ),
    (ProviderKind::Kubernetes, ResourceKind::KubernetesWorkload) => (
      &[][..],
      StreamingCapabilities {
        logs: UnsupportedHidden,
        terminal: UnsupportedHidden,
      },
    ),
    (ProviderKind::Kubernetes, ResourceKind::KubernetesPod) => (
      &[][..],
      StreamingCapabilities {
        logs: Enabled,
        terminal: Enabled,
      },
    ),
    _ => return None,
  };
  let state = |action| {
    if enabled.contains(&action) {
      Enabled
    } else {
      UnsupportedDisabled
    }
  };
  Some(ProviderCapabilities {
    actions: ActionCapabilities {
      deploy: state(Deploy),
      pull: state(Pull),
      start: state(Start),
      restart: state(Restart),
      pause: state(Pause),
      unpause: state(Unpause),
      stop: state(Stop),
      delete: state(Delete),
    },
    streaming,
  })
}
