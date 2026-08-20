//! Render an Alerter's message template for one alert.
//!
//! Deliberately not a templating engine: `{{path}}` substitution over
//! the alert's own JSON, no conditionals, no loops. A notification is
//! a sentence with values in it, and the alternative was a
//! general-purpose engine plus its dependency to write one.

use komodo_client::entities::alert::{Alert, AlertDataVariant};

/// The template configured for this alert's type, if any, rendered.
///
/// `None` means "use the built-in formatter", which is both the
/// no-template case and the empty-template case.
pub fn rendered_template(
  templates: &[komodo_client::entities::alerter::AlertTemplate],
  alert: &Alert,
) -> Option<String> {
  let variant: AlertDataVariant = (&alert.data).into();
  let template = templates
    .iter()
    .find(|template| template.alert_type == variant)
    .map(|template| template.template.trim())
    .filter(|template| !template.is_empty())?;
  Some(render(template, alert))
}

fn render(template: &str, alert: &Alert) -> String {
  let context = context(alert);
  let mut out = String::with_capacity(template.len());
  let mut rest = template;
  while let Some(start) = rest.find("{{") {
    out.push_str(&rest[..start]);
    let after = &rest[start + 2..];
    let Some(end) = after.find("}}") else {
      // No closing braces: the rest is literal text that happens to
      // contain `{{`.
      out.push_str(&rest[start..]);
      return out;
    };
    let path = after[..end].trim();
    match lookup(&context, path) {
      Some(value) => out.push_str(&value),
      // Left as written. A placeholder that silently renders empty is
      // a template the author cannot debug from the message.
      None => {
        out.push_str("{{");
        out.push_str(&after[..end]);
        out.push_str("}}");
      }
    }
    rest = &after[end + 2..];
  }
  out.push_str(rest);
  out
}

/// The values a template can name: the alert's own fields, plus every
/// field of its data flattened to the top level so `{{name}}` works
/// without knowing the variant's shape.
fn context(alert: &Alert) -> serde_json::Value {
  let (resource_type, resource_id) =
    alert.target.extract_variant_id();
  let mut map = serde_json::Map::new();
  map.insert(
    String::from("level"),
    serde_json::Value::String(format!("{:?}", alert.level)),
  );
  map.insert(
    String::from("resolved"),
    serde_json::Value::Bool(alert.resolved),
  );
  map.insert(
    String::from("ts"),
    serde_json::Value::Number(alert.ts.into()),
  );
  map.insert(
    String::from("resource_type"),
    serde_json::Value::String(format!("{resource_type:?}")),
  );
  map.insert(
    String::from("resource_id"),
    serde_json::Value::String(resource_id.clone()),
  );
  let variant: AlertDataVariant = (&alert.data).into();
  map.insert(
    String::from("alert_type"),
    serde_json::Value::String(format!("{variant:?}")),
  );
  if let Ok(serde_json::Value::Object(data)) =
    serde_json::to_value(&alert.data)
    && let Some(serde_json::Value::Object(fields)) = data.get("data")
  {
    // The tagged representation is { type, data }, and the fields
    // worth naming are inside `data`. Inserted without overwriting,
    // so `{{level}}` always means the alert's level.
    for (key, value) in fields {
      map.entry(key.clone()).or_insert(value.clone());
    }
  }
  serde_json::Value::Object(map)
}

fn lookup(context: &serde_json::Value, path: &str) -> Option<String> {
  let mut current = context;
  for segment in path.split('.') {
    current = current.get(segment)?;
  }
  Some(match current {
    // Strings unquoted, everything else as JSON. A quoted name in the
    // middle of a sentence is not what anyone meant.
    serde_json::Value::String(value) => value.clone(),
    serde_json::Value::Null => String::new(),
    other => other.to_string(),
  })
}

#[cfg(test)]
mod tests {
  use komodo_client::entities::{
    ResourceTarget,
    alert::{Alert, AlertData, AlertDataVariant, SeverityLevel},
    alerter::AlertTemplate,
  };

  use super::rendered_template;

  fn alert() -> Alert {
    Alert {
      id: Default::default(),
      ts: 1700000000000,
      resolved: false,
      level: SeverityLevel::Critical,
      target: ResourceTarget::Server(String::from("abc123")),
      data: AlertData::ServerUnreachable {
        id: String::from("abc123"),
        name: String::from("data-backend-01"),
        region: None,
        err: None,
      },
      resolved_ts: None,
    }
  }

  fn templates(template: &str) -> Vec<AlertTemplate> {
    vec![AlertTemplate {
      alert_type: AlertDataVariant::ServerUnreachable,
      template: template.to_string(),
    }]
  }

  #[test]
  fn no_template_for_the_variant_falls_back() {
    let other = vec![AlertTemplate {
      alert_type: AlertDataVariant::ServerCpu,
      template: String::from("nope"),
    }];
    assert!(rendered_template(&other, &alert()).is_none());
    assert!(rendered_template(&[], &alert()).is_none());
    // An empty template is a fallback, not an empty message.
    assert!(rendered_template(&templates("  "), &alert()).is_none());
  }

  #[test]
  fn fields_come_from_the_alert_and_its_data() {
    let rendered = rendered_template(
      &templates("{{level}}: {{name}} ({{resource_type}}) is down"),
      &alert(),
    )
    .unwrap();
    assert_eq!(
      rendered,
      "Critical: data-backend-01 (Server) is down"
    );
  }

  #[test]
  fn an_unknown_placeholder_stays_visible() {
    let rendered =
      rendered_template(&templates("{{nope}} {{name}}"), &alert())
        .unwrap();
    assert_eq!(rendered, "{{nope}} data-backend-01");
  }

  #[test]
  fn a_null_field_renders_empty_rather_than_null() {
    let rendered =
      rendered_template(&templates("err=[{{err}}]"), &alert())
        .unwrap();
    assert_eq!(rendered, "err=[]");
  }
}
