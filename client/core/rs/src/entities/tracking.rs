//! Ownership tracking for objects Komodo creates (containers today).
//!
//! Komodo stamps a [TRACKING_LABEL] on every object it creates, encoding
//! the owning resource's identity plus the object's own name. Ownership is
//! resolved from that label instead of a bare name match, so a container
//! copied or renamed by another tool is not silently adopted.

use serde::{Deserialize, Serialize};

use super::ResourceTargetVariant;

/// The label key stamped on every object Komodo creates.
pub const TRACKING_LABEL: &str = "komodo.tracking-id";

/// The parsed contents of a [TRACKING_LABEL] value.
///
/// Serialized form: `<resource_type>/<resource_id>/<object_name>`,
/// eg `Deployment/6a33a0ab2d47fcee6f0909f0/my-container`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackingId {
  /// The type of the Komodo resource which owns the object.
  pub resource_type: ResourceTargetVariant,
  /// The id of the Komodo resource which owns the object.
  pub resource_id: String,
  /// The name the object was created under. Used for the
  /// non-self-referencing check: if the object's actual name differs,
  /// the object is a copy and the label must not confer ownership.
  pub object_name: String,
}

impl TrackingId {
  pub fn new(
    resource_type: ResourceTargetVariant,
    resource_id: impl Into<String>,
    object_name: impl Into<String>,
  ) -> TrackingId {
    TrackingId {
      resource_type,
      resource_id: resource_id.into(),
      object_name: object_name.into(),
    }
  }

  /// The `<resource_type>/<resource_id>/<object_name>` label value.
  pub fn to_label_value(&self) -> String {
    format!(
      "{}/{}/{}",
      self.resource_type, self.resource_id, self.object_name
    )
  }

  /// Parse a label value. Returns None if it isn't a well formed
  /// tracking id, so an unrecognized value never confers ownership.
  pub fn parse(value: &str) -> Option<TrackingId> {
    let mut split = value.splitn(3, '/');
    let resource_type =
      split.next()?.parse::<ResourceTargetVariant>().ok()?;
    let resource_id = split.next()?;
    let object_name = split.next()?;
    if resource_id.is_empty() || object_name.is_empty() {
      return None;
    }
    Some(TrackingId::new(resource_type, resource_id, object_name))
  }

  /// Whether this label was stamped on an object of this exact identity.
  /// A copied / renamed object fails this check.
  pub fn is_self_referencing(&self, object_name: &str) -> bool {
    self.object_name == object_name
  }

  /// Whether this label marks the object as owned by the given resource.
  /// Both the owner identity and the non-self-referencing check must pass.
  pub fn owned_by(
    &self,
    resource_type: ResourceTargetVariant,
    resource_id: &str,
    object_name: &str,
  ) -> bool {
    self.resource_type == resource_type
      && self.resource_id == resource_id
      && self.is_self_referencing(object_name)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn round_trip_and_ownership() {
    let id = TrackingId::new(
      ResourceTargetVariant::Deployment,
      "abc123",
      "my-container",
    );
    let value = id.to_label_value();
    assert_eq!(value, "Deployment/abc123/my-container");
    assert_eq!(TrackingId::parse(&value), Some(id.clone()));

    // Owned by its own resource, under its own name.
    assert!(id.owned_by(
      ResourceTargetVariant::Deployment,
      "abc123",
      "my-container"
    ));
    // Foreign owner.
    assert!(!id.owned_by(
      ResourceTargetVariant::Deployment,
      "other",
      "my-container"
    ));
    // Copied elsewhere: label is not self referencing.
    assert!(!id.is_self_referencing("my-container-copy"));
    assert!(!id.owned_by(
      ResourceTargetVariant::Deployment,
      "abc123",
      "my-container-copy"
    ));
  }

  #[test]
  fn rejects_malformed_values() {
    assert_eq!(TrackingId::parse(""), None);
    assert_eq!(TrackingId::parse("Deployment"), None);
    assert_eq!(TrackingId::parse("Deployment/abc123"), None);
    assert_eq!(TrackingId::parse("Deployment/abc123/"), None);
    assert_eq!(TrackingId::parse("Deployment//name"), None);
    assert_eq!(TrackingId::parse("NotAType/abc123/name"), None);
    // Object names may contain slashes; only the first two are structural.
    assert_eq!(
      TrackingId::parse("Stack/abc123/proj/svc")
        .map(|id| id.object_name),
      Some(String::from("proj/svc"))
    );
  }
}
