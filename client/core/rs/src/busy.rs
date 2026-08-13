use crate::entities::{
  action::ActionActionState, build::BuildActionState,
  cluster::ClusterActionState, deployment::DeploymentActionState,
  procedure::ProcedureActionState, repo::RepoActionState,
  server::ServerActionState, stack::StackActionState,
  swarm::SwarmActionState, sync::ResourceSyncActionState,
  terraform::TerraformActionState,
};

pub trait Busy {
  fn busy(&self) -> bool;
}

impl Busy for ClusterActionState {
  fn busy(&self) -> bool {
    self.deploying
      || self.destroying
      || self.diffing
      || self.applying_object
      || self.deleting_object
      || self.restarting_workload
      || self.rolling_back_workload
      || self.scaling_workload
      || self.cordoning_node
      || self.uncordoning_node
      || self.draining_node
      || self.rolling_back_helm_release
      || self.uninstalling_helm_release
      || self.creating_port_forward
      || self.deleting_port_forward
  }
}

/// Any terraform verb in flight blocks the next one: they share one
/// working directory and one state file, and two applies racing on a
/// single tfstate is how real infrastructure gets duplicated or lost.
impl Busy for TerraformActionState {
  fn busy(&self) -> bool {
    self.initializing
      || self.planning
      || self.applying
      || self.destroying
  }
}

impl Busy for SwarmActionState {
  fn busy(&self) -> bool {
    false
  }
}

impl Busy for ServerActionState {
  fn busy(&self) -> bool {
    self.pruning_containers
      || self.pruning_images
      || self.pruning_networks
      || self.pruning_volumes
      || self.starting_containers > 0
      || self.restarting_containers > 0
      || self.pausing_containers > 0
      || self.unpausing_containers > 0
      || self.stopping_containers > 0
      || self.destroying_containers > 0
  }
}

impl Busy for DeploymentActionState {
  fn busy(&self) -> bool {
    self.deploying
      || self.starting
      || self.restarting
      || self.pausing
      || self.unpausing
      || self.stopping
      || self.destroying
      || self.renaming
  }
}

impl Busy for StackActionState {
  fn busy(&self) -> bool {
    self.deploying
      || self.starting
      || self.restarting
      || self.pausing
      || self.unpausing
      || self.stopping
      || self.destroying
  }
}

impl Busy for BuildActionState {
  fn busy(&self) -> bool {
    self.building
  }
}

impl Busy for RepoActionState {
  fn busy(&self) -> bool {
    self.cloning || self.pulling || self.building
  }
}

impl Busy for ProcedureActionState {
  fn busy(&self) -> bool {
    self.running
  }
}

impl Busy for ActionActionState {
  fn busy(&self) -> bool {
    self.running > 0
  }
}

impl Busy for ResourceSyncActionState {
  fn busy(&self) -> bool {
    self.syncing
  }
}
