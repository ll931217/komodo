//! Refuse a deploy that would take over another resource's container.
//!
//! Komodo stamps `komodo.tracking-id` on everything it creates, and
//! the status code already reads it to decide what a resource owns.
//! This is the write-side counterpart: before a deploy replaces a
//! container, check the container it is about to replace is not
//! stamped for someone else.

use anyhow::anyhow;
use komodo_client::entities::{
  ResourceTargetVariant, deployment::Deployment,
  docker::container::ContainerListItem, server::Server, stack::Stack,
  tracking::TrackingId,
};

use crate::{
  monitor::resources::stack_may_own_container,
  state::server_status_cache,
};

/// The Server's cached container list, or an empty list when no poll
/// has landed yet.
///
/// An empty list means "we do not know", and this check only ever
/// refuses on positive evidence of a foreign owner - so not knowing
/// lets the deploy through rather than blocking on a cold cache.
async fn cached_containers(
  server: &Server,
) -> Vec<ContainerListItem> {
  server_status_cache()
    .get_or_insert_default(&server.id)
    .await
    .docker
    .as_ref()
    .map(|docker| docker.containers.clone())
    .unwrap_or_default()
}

/// Err when the container this Deployment would replace carries a
/// tracking label naming a different resource.
pub async fn check_deployment_not_shared(
  server: &Server,
  deployment: &Deployment,
) -> anyhow::Result<()> {
  if !server.config.fail_on_shared_containers {
    return Ok(());
  }
  let name = deployment.custom_name();
  let Some(container) = cached_containers(server)
    .await
    .into_iter()
    .find(|container| container.name == name)
  else {
    return Ok(());
  };
  let Some(tracking) = container
    .komodo_tracking
    .as_deref()
    .and_then(TrackingId::parse)
  else {
    // No label: created before tracking existed, or by hand. Not
    // evidence of a foreign owner, so not this check's business.
    return Ok(());
  };
  if tracking.resource_type == ResourceTargetVariant::Deployment
    && tracking.resource_id == deployment.id
  {
    return Ok(());
  }
  Err(anyhow!(
    "Refusing to deploy: container '{name}' is stamped for a different resource ({}). Turn off 'fail_on_shared_containers' on Server {}, or rename one of them.",
    container.komodo_tracking.unwrap_or_default(),
    server.name
  ))
}

/// Err when any container matching one of this Stack's services
/// carries a tracking label naming a different resource.
pub async fn check_stack_not_shared(
  server: &Server,
  stack: &Stack,
  service_container_names: &[String],
) -> anyhow::Result<()> {
  if !server.config.fail_on_shared_containers {
    return Ok(());
  }
  let project_name = stack.project_name(false);
  let containers = cached_containers(server).await;
  for container_name in service_container_names {
    let Ok(regex) =
      crate::stack::compose_container_match_regex(container_name)
    else {
      continue;
    };
    for container in &containers {
      if !regex.is_match(&container.name) {
        continue;
      }
      if container.komodo_tracking.is_some()
        && !stack_may_own_container(
          container,
          &stack.id,
          &project_name,
        )
      {
        return Err(anyhow!(
          "Refusing to deploy: container '{}' matches service '{container_name}' but is stamped for a different resource ({}). Turn off 'fail_on_shared_containers' on Server {}, or rename one of them.",
          container.name,
          container.komodo_tracking.clone().unwrap_or_default(),
          server.name
        ));
      }
    }
  }
  Ok(())
}
