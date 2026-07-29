#![allow(unused_crate_dependencies)]

use komodo_client::entities::{
  ResourceTarget,
  provider_contract::{
    CapabilityState, DockerResourceLocator, KubernetesResourceKind,
    KubernetesWorkloadKind, ProviderKind, ResourceAction,
    ResourceIdentity, ResourceKind, ResourceLocator,
    provider_capabilities,
  },
};

const ALL_ACTIONS: [ResourceAction; 8] = [
  ResourceAction::Deploy,
  ResourceAction::Pull,
  ResourceAction::Start,
  ResourceAction::Restart,
  ResourceAction::Pause,
  ResourceAction::Unpause,
  ResourceAction::Stop,
  ResourceAction::Delete,
];

fn assert_actions(
  provider: ProviderKind,
  kind: ResourceKind,
  enabled: &[ResourceAction],
) {
  let capabilities = provider_capabilities(provider, kind).unwrap();

  for action in ALL_ACTIONS {
    let capability = capabilities.action(action);
    assert_eq!(
      capability.enabled(),
      enabled.contains(&action),
      "{provider:?}/{kind:?}/{action:?}"
    );
    assert!(capability.visible());
  }
}

#[test]
fn docker_deployment_capabilities_are_distinct_from_native_containers()
 {
  let managed = ResourceIdentity::from_deployment(
    ProviderKind::Docker,
    ResourceTarget::Deployment("deployment-id".into()),
  )
  .unwrap();

  assert_eq!(managed.kind(), ResourceKind::DockerDeployment);
  assert_actions(
    ProviderKind::Docker,
    ResourceKind::DockerDeployment,
    &ALL_ACTIONS,
  );
  assert_actions(
    ProviderKind::Docker,
    ResourceKind::Container,
    &[
      ResourceAction::Start,
      ResourceAction::Restart,
      ResourceAction::Pause,
      ResourceAction::Unpause,
      ResourceAction::Stop,
      ResourceAction::Delete,
    ],
  );
}

#[test]
fn swarm_capabilities_restrict_actions_and_terminal() {
  for kind in [ResourceKind::SwarmService, ResourceKind::SwarmStack] {
    assert_actions(
      ProviderKind::Swarm,
      kind,
      &[ResourceAction::Deploy, ResourceAction::Delete],
    );
    let streaming = provider_capabilities(ProviderKind::Swarm, kind)
      .unwrap()
      .streaming;
    assert_eq!(streaming.logs, CapabilityState::Enabled);
    assert_eq!(
      streaming.terminal,
      CapabilityState::UnsupportedDisabled
    );
  }
}

#[test]
fn capability_matrix_is_explicit_and_honest() {
  use ResourceAction::*;

  assert_actions(
    ProviderKind::Docker,
    ResourceKind::Container,
    &[Start, Restart, Pause, Unpause, Stop, Delete],
  );
  assert_actions(
    ProviderKind::Docker,
    ResourceKind::DockerDeployment,
    &ALL_ACTIONS,
  );
  assert_actions(
    ProviderKind::Compose,
    ResourceKind::ComposeProject,
    &ALL_ACTIONS,
  );
  assert_actions(
    ProviderKind::Swarm,
    ResourceKind::SwarmService,
    &[Deploy, Delete],
  );
  assert_actions(
    ProviderKind::Swarm,
    ResourceKind::SwarmStack,
    &[Deploy, Delete],
  );
  assert_actions(
    ProviderKind::Kubernetes,
    ResourceKind::KubernetesWorkload,
    &[],
  );
  assert_actions(
    ProviderKind::Kubernetes,
    ResourceKind::KubernetesPod,
    &[],
  );

  for (provider, kind, logs, terminal) in [
    (
      ProviderKind::Docker,
      ResourceKind::Container,
      CapabilityState::Enabled,
      CapabilityState::Enabled,
    ),
    (
      ProviderKind::Docker,
      ResourceKind::DockerDeployment,
      CapabilityState::Enabled,
      CapabilityState::Enabled,
    ),
    (
      ProviderKind::Compose,
      ResourceKind::ComposeProject,
      CapabilityState::Enabled,
      CapabilityState::Enabled,
    ),
    (
      ProviderKind::Swarm,
      ResourceKind::SwarmService,
      CapabilityState::Enabled,
      CapabilityState::UnsupportedDisabled,
    ),
    (
      ProviderKind::Swarm,
      ResourceKind::SwarmStack,
      CapabilityState::Enabled,
      CapabilityState::UnsupportedDisabled,
    ),
    (
      ProviderKind::Kubernetes,
      ResourceKind::KubernetesWorkload,
      CapabilityState::UnsupportedHidden,
      CapabilityState::UnsupportedHidden,
    ),
    (
      ProviderKind::Kubernetes,
      ResourceKind::KubernetesPod,
      CapabilityState::Enabled,
      CapabilityState::Enabled,
    ),
  ] {
    let capabilities = provider_capabilities(provider, kind).unwrap();
    assert_eq!(
      capabilities.streaming.logs, logs,
      "{provider:?}/{kind:?}"
    );
    assert_eq!(
      capabilities.streaming.terminal, terminal,
      "{provider:?}/{kind:?}"
    );
  }

  let valid_pairs = [
    (ProviderKind::Docker, ResourceKind::Container),
    (ProviderKind::Docker, ResourceKind::DockerDeployment),
    (ProviderKind::Compose, ResourceKind::ComposeProject),
    (ProviderKind::Swarm, ResourceKind::SwarmService),
    (ProviderKind::Swarm, ResourceKind::SwarmStack),
    (ProviderKind::Kubernetes, ResourceKind::KubernetesWorkload),
    (ProviderKind::Kubernetes, ResourceKind::KubernetesPod),
  ];
  for provider in [
    ProviderKind::Docker,
    ProviderKind::Compose,
    ProviderKind::Swarm,
    ProviderKind::Kubernetes,
  ] {
    for kind in [
      ResourceKind::Container,
      ResourceKind::DockerDeployment,
      ResourceKind::ComposeProject,
      ResourceKind::SwarmService,
      ResourceKind::SwarmStack,
      ResourceKind::KubernetesWorkload,
      ResourceKind::KubernetesPod,
    ] {
      assert_eq!(
        provider_capabilities(provider, kind).is_some(),
        valid_pairs.contains(&(provider, kind)),
        "{provider:?}/{kind:?}"
      );
    }
  }
}

#[test]
fn action_capabilities_require_every_action_exactly_once() {
  let capabilities = provider_capabilities(
    ProviderKind::Docker,
    ResourceKind::Container,
  )
  .unwrap();
  let json = serde_json::to_value(capabilities.actions).unwrap();
  let object = json.as_object().unwrap();
  assert_eq!(object.len(), ALL_ACTIONS.len());
  for field in [
    "deploy", "pull", "start", "restart", "pause", "unpause", "stop",
    "delete",
  ] {
    assert!(object.contains_key(field), "missing {field}");
  }

  let mut missing = json.clone();
  missing.as_object_mut().unwrap().remove("delete");
  assert!(
    serde_json::from_value::<
      komodo_client::entities::provider_contract::ActionCapabilities,
    >(missing)
    .is_err()
  );
}

#[test]
fn capability_lookup_is_fail_closed_and_visibility_is_explicit() {
  let docker = provider_capabilities(
    ProviderKind::Docker,
    ResourceKind::Container,
  )
  .unwrap();
  assert_eq!(
    docker.action(ResourceAction::Deploy),
    CapabilityState::UnsupportedDisabled
  );
  assert!(!docker.action(ResourceAction::Deploy).enabled());
  assert!(docker.action(ResourceAction::Deploy).visible());

  assert!(CapabilityState::Enabled.enabled());
  assert!(CapabilityState::Enabled.visible());
  assert!(!CapabilityState::UnsupportedDisabled.enabled());
  assert!(CapabilityState::UnsupportedDisabled.visible());
  assert!(!CapabilityState::UnsupportedHidden.enabled());
  assert!(!CapabilityState::UnsupportedHidden.visible());
}

#[test]
fn legacy_deployment_and_stack_targets_round_trip_exactly() {
  let cases = [
    ResourceIdentity::from_deployment(
      ProviderKind::Docker,
      ResourceTarget::Deployment("deployment-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_deployment(
      ProviderKind::Swarm,
      ResourceTarget::Deployment("service-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_stack(
      ProviderKind::Compose,
      ResourceTarget::Stack("stack-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_stack(
      ProviderKind::Swarm,
      ResourceTarget::Stack("swarm-stack-id".into()),
    )
    .unwrap(),
  ];

  for identity in cases {
    let ResourceLocator::Komodo(expected) = identity.locator() else {
      panic!("legacy adapter did not retain ResourceTarget")
    };
    let expected = expected.clone();
    let serialized = serde_json::to_value(&expected).unwrap();
    let target = ResourceTarget::try_from(identity).unwrap();
    assert_eq!(target, expected);
    assert_eq!(serde_json::to_value(&target).unwrap(), serialized);
  }
}

#[test]
fn supported_resource_identities_round_trip_through_json() {
  let cases = [
    ResourceIdentity::docker_container("server-id", "container-name"),
    ResourceIdentity::from_deployment(
      ProviderKind::Docker,
      ResourceTarget::Deployment("docker-deployment-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_deployment(
      ProviderKind::Swarm,
      ResourceTarget::Deployment("swarm-service-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_stack(
      ProviderKind::Compose,
      ResourceTarget::Stack("compose-stack-id".into()),
    )
    .unwrap(),
    ResourceIdentity::from_stack(
      ProviderKind::Swarm,
      ResourceTarget::Stack("swarm-stack-id".into()),
    )
    .unwrap(),
    ResourceIdentity::kubernetes(
      "cluster-id",
      Some("namespace"),
      KubernetesResourceKind::Deployment,
      "workload-name",
    ),
    ResourceIdentity::kubernetes(
      "cluster-id",
      Some("namespace"),
      KubernetesResourceKind::Pod,
      "pod-name",
    ),
  ];

  for identity in cases {
    let serialized = serde_json::to_string(&identity).unwrap();
    let deserialized: ResourceIdentity =
      serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, identity);
  }
}

#[test]
fn resource_identity_serialization_accepts_only_valid_variants() {
  let valid = [
    (
      ResourceIdentity::kubernetes_workload(
        "cluster",
        Some("namespace"),
        KubernetesWorkloadKind::Deployment,
        "api",
      ),
      "KubernetesWorkload",
      "Deployment",
    ),
    (
      ResourceIdentity::kubernetes_pod(
        "cluster",
        Some("namespace"),
        "api-0",
      ),
      "KubernetesPod",
      "Pod",
    ),
  ];
  for (identity, resource_kind, locator_kind) in valid {
    let json = serde_json::to_value(&identity).unwrap();
    assert_eq!(json["resource"]["provider"], "Kubernetes");
    assert_eq!(json["kind"], resource_kind);
    assert_eq!(json["resource"]["locator"]["kind"], locator_kind);
    assert_eq!(
      serde_json::from_value::<ResourceIdentity>(json).unwrap(),
      identity
    );
  }

  for (resource_kind, locator_kind) in [
    ("KubernetesWorkload", "Pod"),
    ("KubernetesPod", "Deployment"),
  ] {
    let malformed = serde_json::json!({
      "provider": "Kubernetes",
      "kind": resource_kind,
      "resource": {
        "provider": "Kubernetes",
        "locator": {
          "cluster": "cluster",
          "namespace": "namespace",
          "kind": locator_kind,
          "name": "api"
        }
      }
    });
    assert!(
      serde_json::from_value::<ResourceIdentity>(malformed).is_err()
    );
  }
}

#[test]
fn all_kubernetes_workload_kinds_round_trip_without_pods() {
  for kind in [
    KubernetesWorkloadKind::Deployment,
    KubernetesWorkloadKind::StatefulSet,
    KubernetesWorkloadKind::DaemonSet,
    KubernetesWorkloadKind::Job,
    KubernetesWorkloadKind::CronJob,
  ] {
    let identity = ResourceIdentity::kubernetes_workload(
      "cluster", None, kind, "workload",
    );
    assert_eq!(identity.kind(), ResourceKind::KubernetesWorkload);
    let json = serde_json::to_value(&identity).unwrap();
    assert_ne!(json["resource"]["locator"]["kind"], "Pod");
    assert_eq!(
      serde_json::from_value::<ResourceIdentity>(json).unwrap(),
      identity
    );
  }
}

#[test]
fn kubernetes_without_namespace_omits_namespace_from_json() {
  let identity = ResourceIdentity::kubernetes(
    "cluster-a",
    None,
    KubernetesResourceKind::Pod,
    "api",
  );
  let serialized = serde_json::to_value(identity).unwrap();

  assert!(
    serialized["resource"]["locator"].get("namespace").is_none()
  );
}

#[test]
fn legacy_adapters_reject_invalid_combinations_without_losing_input()
{
  let target = ResourceTarget::Deployment("deployment-id".into());
  let error = ResourceIdentity::from_deployment(
    ProviderKind::Compose,
    target.clone(),
  )
  .unwrap_err();
  assert_eq!(error.target(), &target);

  let target = ResourceTarget::Stack("stack-id".into());
  let error = ResourceIdentity::from_stack(
    ProviderKind::Docker,
    target.clone(),
  )
  .unwrap_err();
  assert_eq!(error.target(), &target);

  let target = ResourceTarget::Server("server-id".into());
  let error = ResourceIdentity::from_deployment(
    ProviderKind::Docker,
    target.clone(),
  )
  .unwrap_err();
  assert_eq!(error.target(), &target);

  let native = ResourceIdentity::docker_container("server-id", "web");
  assert_eq!(ResourceTarget::try_from(native.clone()), Err(native));
}

#[test]
fn typed_locators_keep_native_identities_distinct() {
  let docker = ResourceIdentity::docker_container("server-a", "web");
  assert_eq!(docker.provider(), ProviderKind::Docker);
  assert_eq!(docker.kind(), ResourceKind::Container);
  assert_eq!(
    docker.locator(),
    ResourceLocator::Docker(DockerResourceLocator {
      server: "server-a".into(),
      name: "web".into(),
    })
  );

  let pod_a = ResourceIdentity::kubernetes(
    "cluster-a",
    Some("production"),
    KubernetesResourceKind::Pod,
    "api",
  );
  let pod_b = ResourceIdentity::kubernetes(
    "cluster-a",
    Some("staging"),
    KubernetesResourceKind::Pod,
    "api",
  );
  let deployment = ResourceIdentity::kubernetes(
    "cluster-a",
    Some("production"),
    KubernetesResourceKind::Deployment,
    "api",
  );
  let other_cluster = ResourceIdentity::kubernetes(
    "cluster-b",
    Some("production"),
    KubernetesResourceKind::Pod,
    "api",
  );
  assert_ne!(pod_a, pod_b);
  assert_ne!(pod_a, deployment);
  assert_ne!(pod_a, other_cluster);
}
